use crate::codec::{FrillsClientToServer, FrillsCodec, FrillsMessage, FrillsServerToClient};
use futures::Stream;
use futures_util::sink::SinkExt;
use futures_util::stream::StreamExt;
use serde::{Deserialize, Serialize};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, IpAddr};
use tokio::net::TcpStream;
use tokio::prelude::*;
use tokio_util::codec::Framed;
use crate::server::{ClientConnectListener, FrillsServer};
use crate::client::FrillsClient;

#[test]
fn bincode() {
    let data = ExampleNestedStructure {
        version: 123,
        value: InternalValue {
            value: r#"{"msg": "Some Fake Data"}"#.to_string(),
        },
    };

    let mut result = match bincode::serialize(&data) {
        Ok(result) => result,
        Err(e) => {
            assert!(false, e);
            return;
        }
    };

    result.push(12u8);
    result.push(34u8);

    let restructured: ExampleNestedStructure = match bincode::deserialize(&result) {
        Ok(output) => output,
        Err(e) => {
            assert!(false, e);
            return;
        }
    };

    assert_eq!(restructured.version, data.version);
    assert_eq!(restructured.value.value, data.value.value);
}

#[test]
fn test_connect_disconnect() {
    let mut runtime = tokio::runtime::Runtime::new().unwrap();

    runtime.block_on(async {
        let mut server = FrillsServer::new(12345);

        tokio::spawn(async {
            let remote = SocketAddr::new(IpAddr::from([0, 0, 0, 0]), 12345);

            let mut client = FrillsClient::new("TestService", remote).await.unwrap();

            let topic_name = "TestTopic";

            client.register_topic(topic_name).await;
            client.subscribe_to_topic(topic_name).await;
            client.push_message(topic_name, "Hello, world!".as_bytes().to_vec()).await;

            let mut message_stream = client.get_message_channel();

            match message_stream.next().await {
                Some(message) => {
                    println!("Message ({}): {}", message.message_id, String::from_utf8(message.message).unwrap());
                },
                _ => {
                    println!("Failed to fetch message");
                }
            }
        });

        server.run().await;
    });
}

#[derive(Serialize, Deserialize)]
struct ExampleNestedStructure {
    pub version: u32,
    pub value: InternalValue,
}

#[derive(Serialize, Deserialize)]
struct InternalValue {
    pub value: String,
}
