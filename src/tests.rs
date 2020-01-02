use crate::client::{FrillsClient, UnAckedFrillsMessage};
use crate::codec::{FrillsClientToServer, FrillsCodec, FrillsMessage, FrillsServerToClient};
use crate::server::{ClientConnectListener, FrillsServer};
use futures::Stream;
use futures_util::sink::SinkExt;
use futures_util::stream::StreamExt;
use serde::{Deserialize, Serialize};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4};
use std::time::{Duration, SystemTime};
use tokio::net::TcpStream;
use tokio::prelude::*;
use tokio_util::codec::Framed;

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
            let mut client = FrillsClient::builder("TestService")
                .cache_size(32)
                .remote_from_str("0.0.0.0:12345")
                .build().await
                .unwrap();

            let mut client_handle = client.get_client_handle();

            client_handle.register_topic("TestTopic").await;
            client_handle.subscribe_to_topic("TestTopic").await;
            client_handle
                .push_messages("TestTopic", vec!["Hello, world!".as_bytes().to_vec()])
                .await;

            match client.next().await {
                Some(message) => {
                    println!(
                        "Message ({}): {}",
                        message.message_id,
                        String::from_utf8(message.message).unwrap()
                    );
                }
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
            let remote = SocketAddr::new(IpAddr::from([127, 0, 0, 1]), 12345);
            let mut producer_client = FrillsClient::builder("Producer")
                .remote_from_str("127.0.0.1:12345")
                .build().await
                .unwrap();

            let mut ack_client = FrillsClient::builder("NACK&ACK")
                .remote_from_str("127.0.0.1:12345")
                .cache_size(16)
                .build().await
                .unwrap();

            let mut producer_handle = producer_client.get_client_handle();
            let mut ack_client_handle = ack_client.get_client_handle();

            let topic = "DataStream";

            ack_client_handle.register_topic(topic).await;
            ack_client_handle.subscribe_to_topic(topic).await;

            // wait a bit to ensure topic was registered
            tokio::time::delay_for(Duration::from_millis(250)).await;

            // push some messages into queue
            let messages: Vec<Vec<u8>> = (0 .. 1_000_000u32).map(|i| format!("{}", i).into_bytes()).collect();

            producer_handle
                .push_messages(topic, messages)
                .await;

            println!("Starting fetch..");

            tokio::spawn(async move {
                let start = SystemTime::now();
                let mut counter = 5u8;
                let mut message_count = 0u32;
                loop {
                    let mut data = Vec::new();

                    for i in 0..10000u32 {
                        let message = match tokio::time::timeout(
                            Duration::from_millis(100),
                            ack_client.next(),
                        )
                        .await
                        {
                            Ok(Some(message)) => message,
                            _ => break,
                        };

                        data.push(message);
                    }

                    println!(
                        "ACK - {} ({})",
                        data.len(),
                        data.last()
                            .unwrap_or(&UnAckedFrillsMessage {
                                message_id: 999,
                                message: vec![69]
                            })
                            .message[0]
                    );

                    message_count += data.len() as u32;

                    if data.is_empty() {
                        counter -= 1;
                    }

                    //ack_client_handle.ack_message_set(data.into_iter().map(|message| message.message_id).collect()).await;

                    if counter == 0 {
                        break;
                    }
                }
                println!(
                    "TIME: {}",
                    SystemTime::now().duration_since(start).unwrap().as_millis()
                );
                println!("TOTAL: {}", message_count);
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
