use crate::client::worker::{FrillsClientTask, FrillsClientWorker};
use crate::codec::{FrillsClientToServer, FrillsCodec, FrillsMessage, FrillsServerToClient};
use crate::tokio::stream::StreamExt;
use crate::utils::next_either;
use futures::future::Either;
use futures::task::{Context, Poll};
use futures::Stream;
use futures_util::sink::SinkExt;
use pin_project::pin_project;
use std::net::SocketAddr;
use std::pin::Pin;
use tokio::net::TcpStream;
use tokio::sync::mpsc::{channel, Receiver, Sender};
use tokio_util::codec::Framed;

#[pin_project]
pub struct FrillsClient {
    service_name: String,
    worker_channel: Sender<FrillsClientTask>,
    worker_broadcast_sender: Sender<UnAckedFrillsMessage>,
    #[pin]
    worker_broadcast_receiver: Receiver<UnAckedFrillsMessage>,
    subscribed: bool,
    message_queue: Vec<UnAckedFrillsMessage>,
}

impl FrillsClient {
    pub async fn new(service_name: &str, remote: SocketAddr) -> Option<Self> {
        let mut remote_stream = match TcpStream::connect(remote).await {
            Ok(stream) => Framed::new(stream, FrillsCodec {}),
            _ => return None,
        };

        let (mut client_sender, client_receiver) = channel(1024);
        let (worker_broadcast_sender, worker_broadcast_receiver) = channel(1024);

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
        };

        Some(new_client)
    }

    pub fn get_client_handle(&self) -> FrillsClientHandle {
        FrillsClientHandle::new(self.worker_channel.clone())
    }
}

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

    pub async fn push_message(&mut self, topic: &str, message: Vec<u8>) {
        self.worker_channel
            .send(FrillsClientTask::PushMessage {
                topic: topic.to_string(),
                message,
            })
            .await;
    }

    pub async fn ack_message(&mut self, message_id: u32) {
        self.worker_channel
            .send(FrillsClientTask::ACKMessage { message_id })
            .await;
    }

    pub async fn ack_message_set(&mut self, message_ids: Vec<u32>) {
        self.worker_channel
            .send(FrillsClientTask::ACKMessageSet { message_ids })
            .await;
    }

    pub async fn nack_message(&mut self, message_id: u32) {
        self.worker_channel
            .send(FrillsClientTask::NACKMessage { message_id })
            .await;
    }

    pub async fn nack_message_set(&mut self, message_ids: Vec<u32>) {
        self.worker_channel
            .send(FrillsClientTask::NACKMessageSet { message_ids })
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
        let mut projection = self.project();

        if projection.message_queue.is_empty() {
            if !*projection.subscribed {
                // register interest
                match projection.worker_channel.poll_ready(cx) {
                    Poll::Ready(Ok(_)) => {
                        cx.waker().wake_by_ref();

                        match projection
                            .worker_channel
                            .try_send(FrillsClientTask::PullMessages { count: 32 })
                        {
                            Ok(_) => {
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
                loop {
                    match projection.worker_broadcast_receiver.poll_recv(cx) {
                        Poll::Ready(Some(message)) => {
                            projection.message_queue.push(message);
                        }
                        _ => {
                            // we're done; receiver is dead
                            //return Poll::Ready(None);
                            *projection.subscribed = false;
                            cx.waker().wake_by_ref();
                            break;
                        }
                    }
                }
            }

            Poll::Pending
        } else {
            Poll::Ready(projection.message_queue.pop())
        }
    }
}

#[derive(Clone)]
pub struct UnAckedFrillsMessage {
    pub message: Vec<u8>,
    pub message_id: u32,
}
