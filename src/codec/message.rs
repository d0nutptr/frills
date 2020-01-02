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
    RegisterTopic { name: String },
    RegisterAsService { name: String },
    SubscribeToTopic { topic_name: String },
    PushMessages { topic: String, messages: Vec<Vec<u8>> },
    PullMessages { count: u32 },
    ACKMessage { message_id: u32 },
    ACKMessageSet { message_ids: Vec<u32> },
    NACKMessage { message_id: u32 },
    NACKMessageSet { message_ids: Vec<u32> },
}

#[derive(Deserialize, Serialize)]
pub enum FrillsServerToClient {
    Empty,
    PulledMessages { messages: Vec<(Vec<u8>, u32)> },
}
