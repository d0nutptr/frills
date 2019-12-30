use tokio::sync::mpsc::Sender;
use tokio::net::TcpStream;

pub enum ClientToMasterMessage {
    RegisterTopic {
        name: String
    },
    RegisterService {
        name: String
    },
    SubscribeServiceToTopic {
        service: String,
        topic: String
    },
    PushMessage {
        topic: String,
        message: Vec<u8>,
    },
    PullMessage {
        service: String,
        client: Sender<NewMessage>
    },
    Ack {
        message_id: u32,
    },
    Nack {
        message_id: u32,
    },
    Disconnect,
    Shutdown,
}

pub struct NewMessage {
    pub message_id: u32,
    pub message: Vec<u8>
}

pub struct NewConnectionNotification {
    pub stream: TcpStream,
}