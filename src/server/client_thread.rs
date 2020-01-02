use crate::codec::{FrillsClientToServer, FrillsCodec, FrillsMessage, FrillsServerToClient};
use crate::server::message::{NewMessage, NewMessages};
use crate::server::ClientToMasterMessage;
use crate::utils::next_either;
use futures::future::Either;
use futures::StreamExt;
use futures_util::sink::SinkExt;
use pin_project::pin_project;
use tokio::net::{TcpListener, TcpStream};
use tokio::prelude::*;
use tokio::sync::mpsc::{channel, Receiver, Sender};
use tokio_util::codec::Framed;

pub struct ClientThread {
    stream: Option<Framed<TcpStream, FrillsCodec>>, // TcpStream
    master_sender: Sender<ClientToMasterMessage>,
    message_receiver: Option<Receiver<NewMessages>>,
    message_sender: Sender<NewMessages>,
    service_name: Option<String>,
    shutdown: bool,
    counter: u32,
}

impl ClientThread {
    pub fn new(stream: TcpStream, sender: Sender<ClientToMasterMessage>) -> Self {
        let (message_sender, message_receiver) = channel(100_000);

        Self {
            stream: Some(Framed::new(stream, FrillsCodec {})),
            master_sender: sender,
            message_receiver: Some(message_receiver),
            message_sender,
            service_name: None,
            shutdown: false,
            counter: 0,
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
            FrillsMessage::ClientToServer(FrillsClientToServer::SubscribeToTopic {
                topic_name,
            }) => {
                self.subscribe_to_topic(topic_name).await;
            }
            FrillsMessage::ClientToServer(FrillsClientToServer::PushMessages { topic, messages }) => {
                self.push_messages(topic, messages).await;
            }
            FrillsMessage::ClientToServer(FrillsClientToServer::PullMessages { count }) => {
                self.pull_messages(count).await;
            }
            FrillsMessage::ClientToServer(FrillsClientToServer::ACKMessage { message_ids }) => {
                self.ack_messages(message_ids).await;
            }
            FrillsMessage::ClientToServer(FrillsClientToServer::NACKMessage { message_ids }) => {
                self.nack_messages(message_ids).await;
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
        self.master_sender
            .send(ClientToMasterMessage::RegisterTopic { name })
            .await;
    }

    async fn register_as_service(&mut self, name: String) {
        match self.service_name {
            Some(_) => {}
            None => {
                self.service_name = Some(name.clone());
                self.master_sender
                    .send(ClientToMasterMessage::RegisterService { name })
                    .await;
            }
        }
    }

    async fn subscribe_to_topic(&mut self, topic_name: String) {
        match &self.service_name {
            Some(service_name) => {
                self.master_sender
                    .send(ClientToMasterMessage::SubscribeServiceToTopic {
                        service: service_name.clone(),
                        topic: topic_name,
                    })
                    .await;
            }
            _ => {}
        }
    }

    async fn push_messages(&mut self, topic: String, messages: Vec<Vec<u8>>) {
        self.master_sender
            .send(ClientToMasterMessage::PushMessages {
                topic,
                messages,
            })
            .await;
    }

    async fn pull_messages(&mut self, count: u32) {
        let service_name = match &self.service_name {
            Some(name) => name.clone(),
            _ => return,
        };

        self.master_sender
            .send(ClientToMasterMessage::PullMessages {
                service: service_name,
                client: self.message_sender.clone(),
                count,
            })
            .await;
    }

    async fn ack_messages(&mut self, message_ids: Vec<u32>) {
        let service = match &self.service_name {
            Some(name) => name.clone(),
            _ => return,
        };

        self.master_sender
            .send(ClientToMasterMessage::ACK {
                message_ids,
                service,
            })
            .await;
    }

    async fn nack_messages(&mut self, message_ids: Vec<u32>) {
        let service = match &self.service_name {
            Some(name) => name.clone(),
            _ => return,
        };

        self.master_sender
            .send(ClientToMasterMessage::NACK {
                message_ids,
                service,
            })
            .await;
    }

    pub async fn perform_master_shutdown(&mut self) {
        self.master_sender
            .send(ClientToMasterMessage::Shutdown)
            .await;
    }

    pub async fn return_message_to_client(&mut self, message: NewMessages) {
        self.send_tcp(FrillsMessage::ServerToClient(
            FrillsServerToClient::PulledMessages {
                messages: message
                    .messages
                    .into_iter()
                    .map(|message| (message.message, message.message_id))
                    .collect(),
            },
        ))
        .await;
    }

    pub async fn run(mut self) {
        while !self.shutdown {
            let (tcp_messages, client_bound_messages) = {
                let mut framed_stream = self.stream.take().unwrap();
                let mut message_stream = self.message_receiver.take().unwrap();

                let mut either = next_either(framed_stream, message_stream);
                let next_message = either.next().await.unwrap();

                // release the streams so we can work with them again (namely, tcp_stream)
                let (framed_stream, message_stream) = either.release();

                self.stream.replace(framed_stream);
                self.message_receiver.replace(message_stream);

                next_message
            };

            for tcp_message in tcp_messages {
                self.process_tcp(tcp_message.unwrap()).await;
            }

            for client_bound_message in client_bound_messages {
                self.return_message_to_client(client_bound_message).await;
            }
        }
    }
}
