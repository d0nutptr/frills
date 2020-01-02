use crate::client::UnAckedFrillsMessage;
use crate::codec::{FrillsClientToServer, FrillsCodec, FrillsMessage, FrillsServerToClient};
use crate::utils::next_either;
use futures_util::sink::SinkExt;
use futures_util::stream::StreamExt;
use tokio::net::TcpStream;
use tokio::sync::mpsc::{channel, Receiver, Sender};
use tokio_util::codec::Framed;

pub(crate) struct FrillsClientWorker {
    remote_stream: Option<Framed<TcpStream, FrillsCodec>>,
    client_receiver: Option<Receiver<FrillsClientTask>>,
    worker_message_broadcast: Sender<UnAckedFrillsMessage>,
    shutdown: bool,
}

impl FrillsClientWorker {
    pub(crate) fn new(
        stream: Framed<TcpStream, FrillsCodec>,
        client_receiver: Receiver<FrillsClientTask>,
        worker_message_broadcast: Sender<UnAckedFrillsMessage>,
    ) -> Self {
        Self {
            remote_stream: Some(stream),
            client_receiver: Some(client_receiver),
            worker_message_broadcast,
            shutdown: false,
        }
    }

    pub(crate) async fn run(&mut self) {
        while !self.shutdown {
            let (remote_messages, client_messages) = {
                let mut remote_stream = self.remote_stream.take().unwrap();
                let mut client_receiver = self.client_receiver.take().unwrap();

                let mut either = next_either(remote_stream, client_receiver);
                let next_message = either.next().await.unwrap();

                let (remote_stream, client_receiver) = either.release();

                self.remote_stream.replace(remote_stream);
                self.client_receiver.replace(client_receiver);

                next_message
            };

            for remote_message in remote_messages {
                self.process_remote_message(remote_message.unwrap()).await;
            }

            for client_message in client_messages {
                self.process_client_message(client_message).await;
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
            FrillsMessage::ServerToClient(FrillsServerToClient::PulledMessages { messages }) => {
                for (message, message_id) in messages {
                    self.handle_pulled_message(message, message_id).await;
                }
            }
            _ => {}
        }
    }

    async fn handle_pulled_message(&mut self, message: Vec<u8>, message_id: u32) {
        self.worker_message_broadcast
            .send(UnAckedFrillsMessage {
                message,
                message_id,
            })
            .await;
    }

    async fn process_client_message(&mut self, message: FrillsClientTask) {
        match message {
            FrillsClientTask::RegisterService { name } => {
                self.register_service(name).await;
            }
            FrillsClientTask::RegisterTopic { name } => {
                self.register_topic(name).await;
            }
            FrillsClientTask::SubscribeToTopic { name } => {
                self.subscribe_to_topic(name).await;
            }
            FrillsClientTask::PushMessage { topic, message } => {
                self.push_message(topic, message).await;
            }
            FrillsClientTask::PullMessages { count } => {
                self.pull_messages(count).await;
            }
            FrillsClientTask::ACKMessage { message_id } => {
                self.ack_message(message_id).await;
            }
            FrillsClientTask::ACKMessageSet { message_ids } => {
                self.ack_message_set(message_ids).await;
            }
            FrillsClientTask::NACKMessage { message_id } => {
                self.nack_message(message_id).await;
            }
            FrillsClientTask::NACKMessageSet { message_ids } => {
                self.nack_message_set(message_ids).await;
            }
            _ => {}
        }
    }

    async fn register_service(&mut self, name: String) {
        self.send_tcp(FrillsMessage::ClientToServer(
            FrillsClientToServer::RegisterAsService { name },
        ))
        .await;
    }

    async fn register_topic(&mut self, name: String) {
        self.send_tcp(FrillsMessage::ClientToServer(
            FrillsClientToServer::RegisterTopic { name },
        ))
        .await;
    }

    async fn subscribe_to_topic(&mut self, name: String) {
        self.send_tcp(FrillsMessage::ClientToServer(
            FrillsClientToServer::SubscribeToTopic { topic_name: name },
        ))
        .await;
    }

    async fn push_message(&mut self, topic: String, message: Vec<u8>) {
        self.send_tcp(FrillsMessage::ClientToServer(
            FrillsClientToServer::PushMessage { topic, message },
        ))
        .await;
    }

    async fn pull_messages(&mut self, count: u32) {
        self.send_tcp(FrillsMessage::ClientToServer(
            FrillsClientToServer::PullMessages { count },
        ))
        .await;
    }

    async fn ack_message(&mut self, message_id: u32) {
        self.send_tcp(FrillsMessage::ClientToServer(
            FrillsClientToServer::ACKMessage { message_id },
        ))
        .await;
    }

    async fn ack_message_set(&mut self, message_ids: Vec<u32>) {
        self.send_tcp(FrillsMessage::ClientToServer(
            FrillsClientToServer::ACKMessageSet { message_ids },
        ))
        .await
    }

    async fn nack_message(&mut self, message_id: u32) {
        self.send_tcp(FrillsMessage::ClientToServer(
            FrillsClientToServer::NACKMessage { message_id },
        ))
        .await;
    }

    async fn nack_message_set(&mut self, message_ids: Vec<u32>) {
        self.send_tcp(FrillsMessage::ClientToServer(
            FrillsClientToServer::NACKMessageSet { message_ids },
        ))
        .await;
    }
}

pub(crate) enum FrillsClientTask {
    RegisterService { name: String },
    RegisterTopic { name: String },
    SubscribeToTopic { name: String },
    PushMessage { topic: String, message: Vec<u8> },
    PullMessages { count: u32 },
    ACKMessage { message_id: u32 },
    ACKMessageSet { message_ids: Vec<u32> },
    NACKMessage { message_id: u32 },
    NACKMessageSet { message_ids: Vec<u32> },
}
