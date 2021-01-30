use crate::server::message::{NewConnectionNotification, NewMessage, NewMessages};
use crate::server::{ClientConnectListener, ClientThread, ClientToMasterMessage};
use crate::utils::next_either;
use futures::future::join_all;
use futures::future::Either;
use futures::StreamExt;
use pin_project::pin_project;
use slab::Slab;
use std::collections::{HashMap, HashSet, VecDeque};
use tokio::sync::mpsc::{channel, Receiver, Sender};
use tokio_stream::wrappers::ReceiverStream;

pub struct FrillsServer {
    connection_receiver: Option<ReceiverStream<NewConnectionNotification>>,
    client_thread_receiver: Option<ReceiverStream<ClientToMasterMessage>>,
    client_thread_sender: Sender<ClientToMasterMessage>,
    services: HashMap<String, Service>,
    topics: HashMap<String, Topic>,
    shutdown: bool,
    count: u32,
}

impl FrillsServer {
    pub fn new(port: u16) -> Self {
        let (connection_sender, connection_receiver) = channel(16);
        let (client_thread_sender, client_thread_receiver) = channel(16);

        // spawn connection listener
        tokio::spawn(async move {
            let connection_listener = ClientConnectListener::new(port, connection_sender).await;

            connection_listener.listen().await;
        });

        Self {
            connection_receiver: Some(ReceiverStream::new(connection_receiver)),
            client_thread_receiver: Some(ReceiverStream::new(client_thread_receiver)),
            client_thread_sender,
            shutdown: false,
            services: HashMap::new(),
            topics: HashMap::new(),
            count: 0,
        }
    }

    pub async fn run(mut self) {
        while !self.shutdown {
            let (connection_received_messages, client_thread_messages) = {
                let mut connection_receiver = self.connection_receiver.take().unwrap();
                let mut client_thread_receiver = self.client_thread_receiver.take().unwrap();

                let mut either = next_either(connection_receiver, client_thread_receiver);
                let next_message = either.next().await.unwrap();

                // release the streams so we can work with them again (namely, tcp_stream)
                let (released_connection_receiver, released_client_thread_receiver) =
                    either.release();

                self.connection_receiver
                    .replace(released_connection_receiver);
                self.client_thread_receiver
                    .replace(released_client_thread_receiver);

                next_message
            };

            for connection_received_message in connection_received_messages {
                self.create_new_client(connection_received_message).await;
            }

            for client_thread_message in client_thread_messages {
                self.process_client_message(client_thread_message).await;
            }
        }
    }

    async fn create_new_client(&mut self, new_connection_notification: NewConnectionNotification) {
        let client = ClientThread::new(
            new_connection_notification.stream,
            self.client_thread_sender.clone(),
        );
        tokio::spawn(client.run());
    }

    async fn process_client_message(&mut self, message: ClientToMasterMessage) {
        match message {
            ClientToMasterMessage::Shutdown => {
                self.perform_shutdown();
            }
            ClientToMasterMessage::RegisterTopic { name } => {
                self.register_topic(name);
            }
            ClientToMasterMessage::RegisterService { name } => {
                self.register_service(name);
            }
            ClientToMasterMessage::SubscribeServiceToTopic { topic, service } => {
                self.subscribe_service_to_topic(topic, service);
            }
            ClientToMasterMessage::PushMessages { topic, messages } => {
                self.push_messages(topic, messages).await;
            }
            ClientToMasterMessage::PullMessages {
                service,
                client,
                count,
            } => {
                self.pull_message(service, client, count).await;
            }
            ClientToMasterMessage::ACK {
                service,
                message_ids,
            } => {
                self.ack_messages(service, message_ids).await;
            }
            ClientToMasterMessage::NACK {
                service,
                message_ids,
            } => {
                self.nack_messages(service, message_ids).await;
            }
            _ => {}
        };
    }

    fn perform_shutdown(&mut self) {
        self.shutdown = true;
    }

    fn register_topic(&mut self, name: String) {
        if !self.topics.contains_key(&name) {
            self.topics.insert(name, Topic::new());
        }
    }

    fn register_service(&mut self, name: String) {
        if !self.services.contains_key(&name) {
            self.services.insert(name, Service::new());
        }
    }

    fn subscribe_service_to_topic(&mut self, topic_name: String, service_name: String) {
        let topic = match self.topics.get_mut(&topic_name) {
            Some(topic) => topic,
            None => return,
        };

        if self.services.contains_key(&service_name) {
            topic.register_service(service_name);
        }
    }

    async fn push_messages(&mut self, topic: String, messages: Vec<Vec<u8>>) {
        let services: Vec<&String> = match self.topics.get(&topic) {
            Some(topic) => topic.registered_services.iter().collect(),
            _ => return,
        };

        let mapped_messages: Vec<Message> = messages.into_iter()
            .map(|message| {
                Message {
                    data: message.clone(),
                }
            })
            .collect();

        for service_name in services {
            let service = match self.services.get_mut(service_name) {
                Some(service) => service,
                _ => continue,
            };

            service.enqueue_messages(mapped_messages.clone()).await;
        }
    }

    async fn pull_message(&mut self, service: String, client: Sender<NewMessages>, count: u32) {
        match self.services.get_mut(&service) {
            Some(service) => {
                service.pull_messages(client, count).await;
            }
            _ => {}
        }
    }

    async fn ack_messages(&mut self, service: String, message_ids: Vec<u32>) {
        let service = match self.services.get_mut(&service) {
            Some(service) => service,
            _ => return,
        };

        service.ack_messages(message_ids, false).await;
    }

    async fn nack_messages(&mut self, service: String, message_ids: Vec<u32>) {
        let service = match self.services.get_mut(&service) {
            Some(service) => service,
            _ => return,
        };

        service.ack_messages(message_ids, true).await;
    }
}

struct Topic {
    registered_services: HashSet<String>,
}

impl Topic {
    fn new() -> Self {
        Self {
            registered_services: HashSet::new(),
        }
    }

    fn register_service(&mut self, service_name: String) {
        self.registered_services.insert(service_name);
    }
}

struct Service {
    registered_clients: HashSet<u32>,
    unsatisfied_messages: Slab<Message>,
    enqueued_messages: VecDeque<Message>,
}

impl Service {
    fn new() -> Self {
        Self {
            registered_clients: HashSet::new(),
            unsatisfied_messages: Slab::with_capacity(10_000),
            enqueued_messages: VecDeque::new(),
        }
    }

    fn register_client(&mut self, client_id: u32) {
        self.registered_clients.insert(client_id);
    }

    async fn enqueue_messages(&mut self, messages: Vec<Message>) {
        self.enqueued_messages.extend(messages);
    }

    async fn pull_messages(&mut self, mut client: Sender<NewMessages>, count: u32) {
        if self.enqueued_messages.is_empty() {
            tokio::spawn(async move {
                client.send(NewMessages { messages: vec![] }).await;
            });
        } else {
            let mut messages = Vec::new();

            for _ in 0..count {
                match self.enqueued_messages.pop_front() {
                    Some(message) => {
                        let message_id = self.unsatisfied_messages.insert(message.clone()) as u32;

                        messages.push(NewMessage {
                            message_id,
                            message: message.data,
                        });
                    }
                    None => break,
                }
            }

            tokio::spawn(async move { client.send(NewMessages { messages }).await });
        }
    }

    async fn ack_messages(&mut self, message_ids: Vec<u32>, is_nack: bool) {
        let valid_ids: Vec<usize> = message_ids.into_iter()
            .map(|id| id as usize)
            .filter(|id| self.unsatisfied_messages.contains(*id))
            .collect();

        let pending_messages: Vec<Message> = valid_ids.into_iter()
            .map(|id| self.unsatisfied_messages.remove(id))
            .collect();

        if is_nack {
            // NACK; requeue
            self.enqueue_messages(pending_messages).await;
        }
    }
}

#[derive(Clone)]
struct Message {
    data: Vec<u8>,
}
