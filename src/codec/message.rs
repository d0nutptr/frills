use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize)]
pub enum FrillsMessage {
    ClientToServer(FrillsClientToServer),
    ServerToClient(FrillsServerToClient),
}

#[derive(Deserialize, Serialize)]
pub enum FrillsClientToServer {
    Disconnect,
    Shutdown,
    RegisterTopic {
        name: String
    },
    RegisterAsService {
        name: String
    },
    SubscribeToTopic {
        topic_name: String
    },
    PushMessage {
        topic: String,
        message: Vec<u8>
    },
    PullMessage,
    ACKMessage {
        message_id: u32
    },
    NACKMessage {
        message_id: u32
    }
}

#[derive(Deserialize, Serialize)]
pub enum FrillsServerToClient {
    Empty,
    PulledMessage {
        message: Vec<u8>,
        message_id: u32
    }
}