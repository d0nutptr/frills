use pin_project::pin_project;
use tokio::net::{TcpListener, TcpStream};
use tokio::prelude::*;
use tokio::sync::mpsc::{channel, Receiver, Sender};
use tokio_util::codec::Framed;
use crate::codec::{FrillsMessage, FrillsServerToClient, FrillsClientToServer, FrillsCodec};
use futures::future::Either;
use crate::utils::next_either;
use futures_util::sink::SinkExt;
use futures::StreamExt;
use crate::server::ClientToMasterMessage;
use crate::server::message::NewMessage;

pub struct ClientThread {
    stream: Option<Framed<TcpStream, FrillsCodec>>, // TcpStream
    master_sender: Sender<ClientToMasterMessage>,
    message_receiver: Option<Receiver<NewMessage>>,
    message_sender: Sender<NewMessage>,
    service_name: Option<String>,
    shutdown: bool,
}

impl ClientThread {
    pub fn new(
        stream: TcpStream,
        sender: Sender<ClientToMasterMessage>
    ) -> Self {
        let (message_sender, message_receiver) = channel(100_000);

        Self {
            stream: Some(Framed::new(stream, FrillsCodec {})),
            master_sender: sender,
            message_receiver: Some(message_receiver),
            message_sender,
            service_name: None,
            shutdown: false,
        }
    }

    async fn process_tcp(&mut self, message: FrillsMessage) {
        match message {
            FrillsMessage::ClientToServer(FrillsClientToServer::Disconnect) => {
                // todo tell the master since we have shit to do as a result
                self.shutdown = true;
            }
            FrillsMessage::ClientToServer(FrillsClientToServer::Shutdown) => {
                self.perform_master_shutdown().await;
            }
            FrillsMessage::ClientToServer(FrillsClientToServer::RegisterTopic { name }) => {
                self.register_topic(name).await;
            }
            FrillsMessage::ClientToServer(FrillsClientToServer::RegisterAsService { name }) => {
                self.register_as_service(name).await;
            }
            FrillsMessage::ClientToServer(FrillsClientToServer::SubscribeToTopic { topic_name }) => {
                self.subscribe_to_topic(topic_name).await;
            }
            FrillsMessage::ClientToServer(FrillsClientToServer::PushMessage { topic, message }) => {
                self.push_message(topic, message).await;
            }
            FrillsMessage::ClientToServer(FrillsClientToServer::PullMessage) => {
                self.pull_message().await;
            }
            FrillsMessage::ClientToServer(FrillsClientToServer::ACKMessage { message_id }) => {
                self.ack_message(message_id).await;
            }
            FrillsMessage::ClientToServer(FrillsClientToServer::NACKMessage { message_id }) => {
                self.nack_message(message_id).await;
            }
            _ => {
                println!("Other message type received!");
            }
        }
    }

    async fn send_tcp(&mut self, message: FrillsMessage) {
        let mut framed_stream = self.stream.take().unwrap();

        framed_stream.send(message).await;

        self.stream.replace(framed_stream);
    }

    async fn register_topic(&mut self, name: String) {
        self.master_sender.send(ClientToMasterMessage::RegisterTopic { name }).await;
    }

    async fn register_as_service(&mut self, name: String) {
        match self.service_name {
            Some(_) => {},
            None => {
                self.service_name = Some(name.clone());
                self.master_sender.send(ClientToMasterMessage::RegisterService { name }).await;
            }
        }
    }

    async fn subscribe_to_topic(&mut self, topic_name: String) {
        match &self.service_name {
            Some(service_name) => {
                self.master_sender.send(ClientToMasterMessage::SubscribeServiceToTopic { service: service_name.clone(), topic: topic_name }).await;
            },
            _ => {}
        }
    }

    async fn push_message(&mut self, topic: String, data: Vec<u8>) {
        self.master_sender.send(ClientToMasterMessage::PushMessage { topic, message: data }).await;
    }

    async fn pull_message(&mut self) {
        let service_name = match &self.service_name {
            Some(name) => name.clone(),
            _ => return
        };

        self.master_sender.send(ClientToMasterMessage::PullMessage { service: service_name, client: self.message_sender.clone() }).await;
    }

    async fn ack_message(&mut self, message_id: u32) {
        let service = match &self.service_name {
            Some(name) => name.clone(),
            _ => return
        };

        self.master_sender.send(ClientToMasterMessage::ACK { message_id, service }).await;
    }

    async fn nack_message(&mut self, message_id: u32) {
        let service = match &self.service_name {
            Some(name) => name.clone(),
            _ => return
        };

        self.master_sender.send(ClientToMasterMessage::NACK { message_id, service }).await;
    }

    pub async fn perform_master_shutdown(&mut self) {
        self.master_sender.send(ClientToMasterMessage::Shutdown).await;
    }

    pub async fn return_message_to_client(&mut self, message: NewMessage) {
        self.send_tcp(FrillsMessage::ServerToClient(FrillsServerToClient::PulledMessage {
            message: message.message,
            message_id: message.message_id
        })).await;
    }

    pub async fn run(mut self) {
        while !self.shutdown {
            let next_message = {
                let mut framed_stream = self.stream.take().unwrap();
                let mut message_stream = self.message_receiver.take().unwrap();;

                let mut either = next_either(framed_stream, message_stream);
                let next_message = either.next().await.unwrap();

                // release the streams so we can work with them again (namely, tcp_stream)
                let (framed_stream, message_stream) = either.release();

                self.stream.replace(framed_stream);
                self.message_receiver.replace(message_stream);

                next_message
            };

            match next_message {
                // New Connection
                Either::Left(message) => {
                    self.process_tcp(message.unwrap()).await;
                }
                // New client thread
                Either::Right(message) => {
                    self.return_message_to_client(message).await;
                }
            };
        }
    }
}
