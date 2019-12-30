use crate::codec::{FrillsCodec, FrillsMessage, FrillsClientToServer, FrillsServerToClient};
use tokio_util::codec::Framed;
use tokio::net::TcpStream;
use std::net::SocketAddr;
use futures_util::sink::SinkExt;
use pin_project::pin_project;
use crate::tokio::stream::StreamExt;
use tokio::sync::mpsc::{Sender, Receiver, channel};
use tokio::sync::broadcast;
use crate::utils::next_either;
use futures::future::Either;
use futures::Stream;
use futures::task::{Context, Poll};
use std::pin::Pin;

pub struct FrillsClient {
    service_name: String,
    worker_channel: Sender<FrillsClientTask>,
    worker_broadcast_sender: broadcast::Sender<UnAckedFrillsMessage>,
    worker_broadcast_receiver: broadcast::Receiver<UnAckedFrillsMessage>
}

impl FrillsClient {
    pub async fn new(service_name: &str, remote: SocketAddr) -> Option<Self> {
        let mut remote_stream = match TcpStream::connect(remote).await {
            Ok(stream) => Framed::new(stream, FrillsCodec {}),
            _ => return None
        };

        let (client_sender, client_receiver) = channel(1);
        let (worker_broadcast_sender, worker_broadcast_receiver) = broadcast::channel(64);

        let cloned_worker_sender = worker_broadcast_sender.clone();

        tokio::spawn(async move {
            let mut worker = FrillsClientWorker::new(remote_stream, client_receiver, cloned_worker_sender);
            worker.run().await;
        });

        let mut new_client = Self {
            service_name: service_name.to_string(),
            worker_channel: client_sender,
            worker_broadcast_sender,
            worker_broadcast_receiver
        };

        new_client.register_service().await;

        Some(new_client)
    }

    async fn register_service(&mut self) {
        self.worker_channel.send(FrillsClientTask::RegisterService { name: self.service_name.clone() }).await;
    }

    pub async fn register_topic(&mut self, topic: &str) {
        self.worker_channel.send(FrillsClientTask::RegisterTopic { name: topic.to_string() }).await;
    }

    pub async fn subscribe_to_topic(&mut self, topic: &str) {
        self.worker_channel.send(FrillsClientTask::SubscribeToTopic { name: topic.to_string() }).await;
    }

    pub async fn push_message(&mut self, topic: &str, message: Vec<u8>) {
        self.worker_channel.send(FrillsClientTask::PushMessage { topic: topic.to_string(), message }).await;
    }

    pub fn get_message_channel(&self) -> FrillsClientMessageStream {
        FrillsClientMessageStream::new(self.worker_channel.clone(), self.worker_broadcast_sender.subscribe())
    }
}

struct FrillsClientWorker {
    remote_stream: Option<Framed<TcpStream, FrillsCodec>>,
    client_receiver: Option<Receiver<FrillsClientTask>>,
    worker_message_broadcast: broadcast::Sender<UnAckedFrillsMessage>,
    shutdown: bool
}

impl FrillsClientWorker {
    fn new(stream: Framed<TcpStream, FrillsCodec>, client_receiver: Receiver<FrillsClientTask>, worker_message_broadcast: broadcast::Sender<UnAckedFrillsMessage>) -> Self {
        Self {
            remote_stream: Some(stream),
            client_receiver: Some(client_receiver),
            worker_message_broadcast,
            shutdown: false
        }
    }

    async fn run(&mut self) {
        while !self.shutdown {
            let next_message = {
                let mut remote_stream = self.remote_stream.take().unwrap();
                let mut client_receiver = self.client_receiver.take().unwrap();

                let mut either = next_either(remote_stream, client_receiver);
                let next_message = either.next().await.unwrap();

                let (remote_stream, client_receiver) = either.release();

                self.remote_stream.replace(remote_stream);
                self.client_receiver.replace(client_receiver);

                next_message
            };

            match next_message {
                Either::Left(message) => {
                    // maybe handle this better just in case we get shitty bytes
                    self.process_remote_message(message.unwrap()).await;
                },
                Either::Right(message) => {
                    self.process_client_message(message).await;
                }
            }
        }
    }

    async fn send_tcp(&mut self, message: FrillsMessage) {
        let mut remote_stream = self.remote_stream.take().unwrap();

        remote_stream.send(message).await;

        self.remote_stream.replace(remote_stream);
    }

    async fn process_remote_message(&mut self, message: FrillsMessage) {
        match message {
            FrillsMessage::ServerToClient(FrillsServerToClient::PulledMessage { message, message_id}) => {
                self.handle_pulled_message(message, message_id);
            },
            _ => {}
        }
    }

    fn handle_pulled_message(&mut self, message: Vec<u8>, message_id: u32) {
        self.worker_message_broadcast.send(UnAckedFrillsMessage {
            message,
            message_id
        });
    }

    async fn process_client_message(&mut self, message: FrillsClientTask) {
        match message {
            FrillsClientTask::RegisterService { name} => {
                self.register_service(name).await;
            },
            FrillsClientTask::RegisterTopic { name } => {
                self.register_topic(name).await;
            },
            FrillsClientTask::SubscribeToTopic { name } => {
                self.subscribe_to_topic(name).await;
            },
            FrillsClientTask::PushMessage { topic, message} => {
                self.push_message(topic, message).await;
            }
            FrillsClientTask::PullMessage => {
                self.pull_message().await;
            },
            _ => {}
        }
    }

    async fn register_service(&mut self, name: String) {
        self.send_tcp(FrillsMessage::ClientToServer(FrillsClientToServer::RegisterAsService { name })).await;
    }

    async fn register_topic(&mut self, name: String) {
        self.send_tcp(FrillsMessage::ClientToServer(FrillsClientToServer::RegisterTopic { name })).await;
    }

    async fn subscribe_to_topic(&mut self, name: String) {
        self.send_tcp(FrillsMessage::ClientToServer(FrillsClientToServer::SubscribeToTopic { topic_name: name })).await;
    }

    async fn push_message(&mut self, topic: String, message: Vec<u8>) {
        self.send_tcp(FrillsMessage::ClientToServer(FrillsClientToServer::PushMessage { topic, message })).await;
    }

    async fn pull_message(&mut self) {
        self.send_tcp(FrillsMessage::ClientToServer(FrillsClientToServer::PullMessage)).await;
    }
}

#[pin_project]
pub struct FrillsClientMessageStream {
    #[pin]
    receiver: broadcast::Receiver<UnAckedFrillsMessage>,
    #[pin]
    sender: Sender<FrillsClientTask>,
    subscribed: bool
}

impl FrillsClientMessageStream {
    fn new(sender: Sender<FrillsClientTask>, receiver: broadcast::Receiver<UnAckedFrillsMessage>) -> Self {
        Self {
            sender,
            receiver,
            subscribed: false
        }
    }
}

impl Stream for FrillsClientMessageStream {
    type Item = UnAckedFrillsMessage;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut projection = self.project();

        if !*projection.subscribed {
            // register interest
            match projection.sender.poll_ready(cx) {
                Poll::Ready(Ok(_)) => {
                    *projection.subscribed = true;
                    cx.waker().wake_by_ref();

                    projection.sender.try_send(FrillsClientTask::PullMessage);
                },
                Poll::Ready(Err(_)) => {
                    // we're done; worker is dead
                    return Poll::Ready(None)
                },
                Poll::Pending => { /* still trying to send to worker */ }
            }
        } else {
            // waiting on the value
            match projection.receiver.poll_recv(cx) {
                Poll::Ready(Ok(message)) => {
                    // we'll need to subscribe again to receive another message
                    *projection.subscribed = false;
                    cx.waker().wake_by_ref();

                    return Poll::Ready(Some(message));
                },
                Poll::Ready(Err(_)) => {
                    // we're done; receiver is dead
                    return Poll::Ready(None)
                },
                Poll::Pending => { /* waiting on message */ }
            }
        }

        Poll::Pending
    }
}

enum FrillsClientTask {
    RegisterService {
        name: String
    },
    RegisterTopic {
        name: String
    },
    SubscribeToTopic {
        name: String,
    },
    PushMessage {
        topic: String,
        message: Vec<u8>
    },
    PullMessage
}

#[derive(Clone)]
pub struct UnAckedFrillsMessage {
    pub message: Vec<u8>,
    pub message_id: u32
}