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
use std::time::Duration;

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
            let mut client_handle = client.get_client_handle();

            let topic_name = "TestTopic";

            client_handle.register_topic(topic_name).await;
            client_handle.subscribe_to_topic(topic_name).await;
            client_handle.push_message(topic_name, "Hello, world!".as_bytes().to_vec()).await;

            match client.next().await {
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

#[test]
fn test_nack_requeue() {

    let mut runtime = tokio::runtime::Runtime::new().unwrap();

    runtime.block_on(async {
        let mut server = FrillsServer::new(12345);

        tokio::spawn(async {
            let remote = SocketAddr::new(IpAddr::from([0, 0, 0, 0]), 12345);
            let mut producer_client = FrillsClient::new("Producer", remote).await.unwrap();

            // for the clients to share, they need to have the same service name
            let mut nack_client = FrillsClient::new("NACKAndACK", remote).await.unwrap();
            let mut ack_client = FrillsClient::new("NACKAndACK", remote).await.unwrap();

            let mut nack_client_handle = nack_client.get_client_handle();
            let mut ack_client_handle = ack_client.get_client_handle();

            let topic = "DataStream";

            nack_client_handle.register_topic(topic).await;
            nack_client_handle.subscribe_to_topic(topic).await;

            // shouldn't need to re-register a topic; it already exists
            ack_client_handle.subscribe_to_topic(topic).await;

            // tell the producer to start producing messages so we can get some unique ids
            let mut producer_handle = producer_client.get_client_handle();

            // send 5 messages to give both clients something to do

            for i in 0 .. 100_000_u32 {
                producer_handle.push_message(topic, vec![1]).await;
            }

            tokio::time::delay_for(Duration::from_millis(10_000)).await;
            println!("Starting fetch..");

            tokio::spawn(async move {
                loop {
                    let message = nack_client.next().await.unwrap();

                    println!("NACKED ({}) - {}", message.message_id, message.message[0]);

                    // nack it; we failed! :P
                    nack_client_handle.nack_message(message.message_id).await;
                    break;
                }
            });

            tokio::spawn(async move {
                loop {
                    let mut data = Vec::new();

                    for i in 0 .. 5000u32{
                        let message = match tokio::time::timeout(Duration::from_millis(10), ack_client.next()).await {
                            Ok(Some(message)) => message,
                            _ => break
                        };

                        data.push(message);
                    }

                    for message in data {
                        println!("ACKED ({}) - {}", message.message_id, message.message[0]);

                        ack_client_handle.ack_message(message.message_id).await;
                    }
                }
            });
        });


        // start the server :)
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
