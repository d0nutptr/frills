use crate::client::worker::{FrillsClientTask, FrillsClientWorker};
use crate::codec::{FrillsClientToServer, FrillsCodec, FrillsMessage, FrillsServerToClient};
use crate::utils::next_either;
use futures::future::Either;
use futures::task::{Context, Poll};
use futures::Stream;
use futures_util::stream::StreamExt;
use futures_util::sink::SinkExt;
use pin_project::pin_project;
use std::net::SocketAddr;
use std::pin::Pin;
use tokio::net::TcpStream;
use tokio::sync::mpsc::{channel, Receiver, Sender};
use tokio_util::codec::Framed;
use std::str::FromStr;

#[pin_project]
pub struct FrillsClient {
    service_name: String,
    worker_channel: Sender<FrillsClientTask>,
    worker_broadcast_sender: Sender<Vec<UnAckedFrillsMessage>>,
    #[pin]
    worker_broadcast_receiver: Receiver<Vec<UnAckedFrillsMessage>>,
    message_queue: Vec<UnAckedFrillsMessage>,
    subscribed: bool,
    cache_size: u16
}

impl FrillsClient {
    pub fn builder(service_name: &str) -> Builder {
        Builder::new(service_name)
    }

    async fn new(config: FrillsClientConfig) -> Option<Self> {
        let remote = config.remote;
        let cache_size = config.cache_size;
        let service_name = config.service_name;

        let mut remote_stream = match TcpStream::connect(remote).await {
            Ok(stream) => Framed::new(stream, FrillsCodec {}),
            _ => return None,
        };

        let (mut client_sender, client_receiver) = channel(16);
        let (worker_broadcast_sender, worker_broadcast_receiver) = channel(16);

        let cloned_worker_sender = worker_broadcast_sender.clone();

        tokio::spawn(async move {
            let mut worker =
                FrillsClientWorker::new(remote_stream, client_receiver, cloned_worker_sender);
            worker.run().await;
        });

        client_sender
            .send(FrillsClientTask::RegisterService {
                name: service_name.to_string(),
            })
            .await;

        let mut new_client = Self {
            service_name: service_name.to_string(),
            worker_channel: client_sender,
            worker_broadcast_sender,
            worker_broadcast_receiver,
            subscribed: false,
            message_queue: Vec::new(),
            cache_size
        };

        Some(new_client)
    }

    pub fn get_client_handle(&self) -> FrillsClientHandle {
        FrillsClientHandle::new(self.worker_channel.clone())
    }
}

#[derive(Clone)]
pub struct UnAckedFrillsMessage {
    pub message: Vec<u8>,
    pub message_id: u32,
}

#[derive(Clone)]
pub struct FrillsClientHandle {
    worker_channel: Sender<FrillsClientTask>,
}

impl FrillsClientHandle {
    fn new(worker_channel: Sender<FrillsClientTask>) -> Self {
        Self { worker_channel }
    }

    pub async fn register_topic(&mut self, topic: &str) {
        self.worker_channel
            .send(FrillsClientTask::RegisterTopic {
                name: topic.to_string(),
            })
            .await;
    }

    pub async fn subscribe_to_topic(&mut self, topic: &str) {
        self.worker_channel
            .send(FrillsClientTask::SubscribeToTopic {
                name: topic.to_string(),
            })
            .await;
    }

    pub async fn push_messages(&mut self, topic: &str, messages: Vec<Vec<u8>>) {
        self.worker_channel
            .send(FrillsClientTask::PushMessages {
                topic: topic.to_string(),
                messages,
            })
            .await;
    }

    pub async fn ack_message(&mut self, message_ids: Vec<u32>) {
        self.worker_channel
            .send(FrillsClientTask::ACKMessages { message_ids })
            .await;
    }

    pub async fn nack_message(&mut self, message_ids: Vec<u32>) {
        self.worker_channel
            .send(FrillsClientTask::NACKMessages { message_ids })
            .await;
    }
}

#[pin_project]
pub struct FrillsClientMessageStream {
    #[pin]
    receiver: Receiver<UnAckedFrillsMessage>,
    #[pin]
    sender: Sender<FrillsClientTask>,
    subscribed: bool,
}

impl FrillsClientMessageStream {
    fn new(sender: Sender<FrillsClientTask>, receiver: Receiver<UnAckedFrillsMessage>) -> Self {
        Self {
            sender,
            receiver,
            subscribed: false,
        }
    }
}

impl Stream for FrillsClient {
    type Item = UnAckedFrillsMessage;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let cache_size = self.cache_size as u32;
        let mut projection = self.project();

        if projection.message_queue.is_empty() {
            if !*projection.subscribed {
                // register interest
                match projection.worker_channel.poll_ready(cx) {
                    Poll::Ready(Ok(_)) => {

                        match projection
                            .worker_channel
                            .try_send(FrillsClientTask::PullMessages { count: cache_size })
                        {
                            Ok(_) => {
                                cx.waker().wake_by_ref();
                                *projection.subscribed = true;
                            }
                            _ => {}
                        };
                    }
                    Poll::Ready(Err(e)) => {
                        // we're done; worker is dead
                        println!("SENDER: {:?}", e);
                        return Poll::Ready(None);
                    }
                    Poll::Pending => { /* still trying to send to worker */ }
                }
            } else {
                // waiting on the value
                match projection.worker_broadcast_receiver.poll_recv(cx) {
                    Poll::Ready(Some(messages)) => {
                        projection.message_queue.extend(messages);
                        *projection.subscribed = false;
                        cx.waker().wake_by_ref();
                    },
                    Poll::Ready(None) => {
                        *projection.subscribed = false;
                        cx.waker().wake_by_ref();
                    },
                    _ => {}
                }
            }

            Poll::Pending
        } else {
            Poll::Ready(projection.message_queue.pop())
        }
    }
}

pub struct Builder {
    config: FrillsClientConfig
}

impl Builder {
    fn new(service_name: &str) -> Self {
        let config = FrillsClientConfig {
            service_name: service_name.to_string(),
            ..Default::default()
        };

        Self {
            config
        }
    }

    pub fn cache_size(mut self, cache_size: u16) -> Self {
        self.config.cache_size = cache_size;
        self
    }

    pub fn remote(mut self, remote: SocketAddr) -> Self {
        self.config.remote = remote;
        self
    }

    /// Panics if `remote` is not a valid SocketAddr
    pub fn remote_from_str(mut self, remote: &str) -> Self {
        self.config.remote = SocketAddr::from_str(remote).unwrap();
        self
    }

    pub async fn build(self) -> Option<FrillsClient> {
        FrillsClient::new(self.config).await
    }
}

pub struct FrillsClientConfig {
    service_name: String,
    cache_size: u16,
    remote: SocketAddr
}

impl Default for FrillsClientConfig {
    fn default() -> Self {
        Self {
            service_name: "ClientService".to_string(),
            cache_size: 16,
            remote: SocketAddr::from_str("127.0.0.1:0").unwrap()
        }
    }
}