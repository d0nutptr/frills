use tokio::net::TcpStream;
use tokio::sync::mpsc::Sender;

pub enum ClientToMasterMessage {
    RegisterTopic {
        name: String,
    },
    RegisterService {
        name: String,
    },
    SubscribeServiceToTopic {
        service: String,
        topic: String,
    },
    PushMessages {
        topic: String,
        messages: Vec<Vec<u8>>,
    },
    PullMessages {
        service: String,
        client: Sender<NewMessages>,
        count: u32,
    },
    ACK {
        message_ids: Vec<u32>,
        service: String,
    },
    NACK {
        message_ids: Vec<u32>,
        service: String,
    },
    Disconnect,
    Shutdown,
}

pub struct NewMessages {
    pub messages: Vec<NewMessage>,
}

pub struct NewMessage {
    pub message_id: u32,
    pub message: Vec<u8>,
}

pub struct NewConnectionNotification {
    pub stream: TcpStream,
}
