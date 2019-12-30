use tokio::sync::mpsc::{channel, Receiver, Sender};
use crate::server::{ClientConnectListener, ClientThread, ClientToMasterMessage};
use pin_project::pin_project;
use crate::server::message::{NewConnectionNotification, NewMessage};
use crate::utils::next_either;
use futures::StreamExt;
use futures::future::Either;
use std::collections::{HashMap, HashSet, VecDeque};
use futures::future::join_all;
use slab::Slab;

pub struct FrillsServer {
    connection_receiver: Option<Receiver<NewConnectionNotification>>,
    client_thread_receiver: Option<Receiver<ClientToMasterMessage>>,
    client_thread_sender: Sender<ClientToMasterMessage>,
    services: HashMap<String, Service>,
    topics: HashMap<String, Topic>,
    shutdown: bool
}

impl FrillsServer {
    pub fn new(port: u16) -> Self {
        let (connection_sender, connection_receiver) = channel(1);
        let (client_thread_sender, client_thread_receiver) = channel(1);

        // spawn connection listener
        tokio::spawn(async move {
            let connection_listener = ClientConnectListener::new(port, connection_sender).await;

            connection_listener.listen().await;
        });

        Self {
            connection_receiver: Some(connection_receiver),
            client_thread_receiver: Some(client_thread_receiver),
            client_thread_sender,
            shutdown: false,
            services: HashMap::new(),
            topics: HashMap::new()
        }
    }

    pub async fn run(mut self) {
        while !self.shutdown {
            let next_message = {
                let mut connection_receiver = self.connection_receiver.take().unwrap();
                let mut client_thread_receiver = self.client_thread_receiver.take().unwrap();

                let mut either = next_either(connection_receiver, client_thread_receiver);
                let next_message = either.next().await.unwrap();

                // release the streams so we can work with them again (namely, tcp_stream)
                let (released_connection_receiver, released_client_thread_receiver) = either.release();

                self.connection_receiver.replace(released_connection_receiver);
                self.client_thread_receiver.replace(released_client_thread_receiver);

                next_message
            };

            match next_message {
                // New Connection
                Either::Left(message) => {
                    self.create_new_client(message).await;
                }
                // New client thread
                Either::Right(message) => {
                    self.process_client_message(message).await;
                }
            };
        }
    }

    async fn create_new_client(&mut self, new_connection_notification: NewConnectionNotification) {
        let client = ClientThread::new(new_connection_notification.stream, self.client_thread_sender.clone());
        tokio::spawn(client.run());
    }

    async fn process_client_message(&mut self, message: ClientToMasterMessage) {
        match message {
            ClientToMasterMessage::Shutdown => {
                self.perform_shutdown();
            },
            ClientToMasterMessage::RegisterTopic { name } => {
                self.register_topic(name);
            },
            ClientToMasterMessage::RegisterService { name } => {
                self.register_service(name);
            },
            ClientToMasterMessage::SubscribeServiceToTopic { topic, service } => {
                self.subscribe_service_to_topic(topic, service);
            },
            ClientToMasterMessage::PushMessage { topic, message } => {
                self.new_message(topic, message).await;
            },
            ClientToMasterMessage::PullMessage { service, client } => {
                self.pull_message(service, client).await;
            },
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
            None => return
        };

        if self.services.contains_key(&service_name) {
            topic.register_service(service_name);
        }
    }

    async fn new_message(&mut self, topic: String, message: Vec<u8>) {
        let services: Vec<&String> = match self.topics.get(&topic) {
            Some(topic) => {
                topic.registered_services.iter().collect()
            },
            _ => { return }
        };

        for service_name in services {
            let service = match self.services.get_mut(service_name) {
                Some(service) => service,
                _ => { continue }
            };

            service.enqueue_message(Message {
                data: message.clone()
            }).await;
        }
    }

    async fn pull_message(&mut self, service: String, client: Sender<NewMessage>) {
        match self.services.get_mut(&service) {
            Some(service) => {
                service.pull_next_message(client.clone()).await;
            },
            _ => {}
        }
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
    pending_clients: VecDeque<Sender<NewMessage>>
}

impl Service {
    fn new() -> Self {
        Self {
            registered_clients: HashSet::new(),
            unsatisfied_messages: Slab::with_capacity(1023),
            enqueued_messages: VecDeque::new(),
            pending_clients: VecDeque::new()
        }
    }

    fn register_client(&mut self, client_id: u32) {
        self.registered_clients.insert(client_id);
    }

    async fn enqueue_message(&mut self, message: Message) {
        if !self.pending_clients.is_empty() {
            let message_id = self.unsatisfied_messages.insert(message.clone());
            let mut client = self.pending_clients.pop_front().unwrap();
            client.send(NewMessage {
                message_id: message_id as u32,
                message: message.data
            }).await;
        } else {
            self.enqueued_messages.push_back(message);
        }
    }

    async fn pull_next_message(&mut self, mut client: Sender<NewMessage>) {
        if self.enqueued_messages.is_empty() {
            self.pending_clients.push_back(client);
        } else {
            let next_message = self.enqueued_messages.pop_front().unwrap();
            let message_id = self.unsatisfied_messages.insert(next_message.clone());

            client.send(NewMessage {
                message_id: message_id as u32,
                message: next_message.data
            }).await;
        }
    }
}

#[derive(Clone)]
struct Message {
    data: Vec<u8>,
}