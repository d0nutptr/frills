use crate::client::worker::{FrillsClientTask, FrillsClientWorker};
use crate::codec::FrillsCodec;
use futures::task::{Context, Poll};
use futures::Stream;
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

        let remote_addrs: Vec<_> = tokio::net::lookup_host(remote).await.ok()?.collect();

        let remote_stream = match TcpStream::connect(&*remote_addrs).await {
            Ok(stream) => Framed::new(stream, FrillsCodec {}),
            _ => return None,
        };

        let (client_sender, client_receiver) = channel(16);
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
            }).await;

        let new_client = Self {
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
        FrillsClientHandle::new(self.get_service_name(), self.worker_channel.clone())
    }

    pub fn get_service_name(&self) -> String {
        self.service_name.clone()
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
    service_name: String
}

impl FrillsClientHandle {
    fn new(service_name: String, worker_channel: Sender<FrillsClientTask>) -> Self {
        Self { service_name, worker_channel }
    }

    pub async fn register_topic(&self, topic: &str) {
        self.worker_channel
            .send(FrillsClientTask::RegisterTopic {
                name: topic.to_string(),
            }).await;
    }

    pub async fn subscribe_to_topic(&self, topic: &str) {
        self.worker_channel
            .send(FrillsClientTask::SubscribeToTopic {
                name: topic.to_string(),
            }).await;
    }

    pub async fn push_messages(&self, topic: &str, messages: Vec<Vec<u8>>) {
        self.worker_channel
            .send(FrillsClientTask::PushMessages {
                topic: topic.to_string(),
                messages,
            }).await;
    }

    pub async fn ack_message(&self, message_ids: Vec<u32>) {
        self.worker_channel
            .send(FrillsClientTask::ACKMessages { message_ids })
            .await;
    }

    pub async fn nack_message(&self, message_ids: Vec<u32>) {
        self.worker_channel
            .send(FrillsClientTask::NACKMessages { message_ids })
            .await;
    }

    pub fn get_service_name(&self) -> String {
        self.service_name.clone()
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
                if projection.worker_channel.is_closed() {
                    return Poll::Ready(None);
                }

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

    pub fn remote(mut self, remote: String) -> Self {
        self.config.remote = remote;
        self
    }

    pub fn remote_from_str(mut self, remote: &str) -> Self {
        self.config.remote = remote.to_string();
        self
    }

    pub async fn build(self) -> Option<FrillsClient> {
        FrillsClient::new(self.config).await
    }
}

pub struct FrillsClientConfig {
    service_name: String,
    cache_size: u16,
    remote: String
}

impl Default for FrillsClientConfig {
    fn default() -> Self {
        Self {
            service_name: "ClientService".to_string(),
            cache_size: 16,
            remote: "127.0.0.1:12345".to_string()
        }
    }
}
