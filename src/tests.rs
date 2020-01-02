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
            let remote = SocketAddr::new(IpAddr::from([0, 0, 0, 0]), 12345);

            let mut client = FrillsClient::new("TestService", remote).await.unwrap();
            let mut client_handle = client.get_client_handle();

            let topic_name = "TestTopic";

            client_handle.register_topic(topic_name).await;
            client_handle.subscribe_to_topic(topic_name).await;
            client_handle
                .push_message(topic_name, "Hello, world!".as_bytes().to_vec())
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
            let mut producer_client = FrillsClient::new("Producer", remote).await.unwrap();

            // for the clients to share, they need to have the same service name
            //let mut nack_client = FrillsClient::new("NACKAndACK", remote).await.unwrap();
            let mut ack_client = FrillsClient::new("NACKAndACK", remote).await.unwrap();
            let mut second_ack_client = FrillsClient::new("NACKAndACK", remote).await.unwrap();

            let mut ack_client_handle = ack_client.get_client_handle();
            let mut second_ack_client_handle = second_ack_client.get_client_handle();

            let topic = "DataStream";

            ack_client_handle.register_topic(topic).await;
            ack_client_handle.subscribe_to_topic(topic).await;

            // tell the producer to start producing messages so we can get some unique ids
            let mut producer_handle = producer_client.get_client_handle();

            tokio::time::delay_for(Duration::from_millis(500)).await;

            // send 5 messages to give both clients something to do

            tokio::spawn(async move {
                for i in 0..1_300_000_u32 {
                    producer_handle
                        .push_message(topic, format!("{}", i).into_bytes())
                        .await;
                }
            });

            tokio::time::delay_for(Duration::from_millis(1000)).await;
            println!("Starting fetch..");

            tokio::spawn(async move {
                let start = SystemTime::now();
                let mut counter = 10u8;
                let mut message_count = 0u32;
                loop {
                    let mut data = Vec::new();

                    for i in 0..10000u32 {
                        let message = match tokio::time::timeout(
                            Duration::from_millis(25),
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

            tokio::spawn(async move {
                let start = SystemTime::now();
                let mut counter = 10u8;
                let mut message_count = 0u32;
                loop {
                    let mut data = Vec::new();

                    for i in 0..10_000_u32 {
                        let message = match tokio::time::timeout(
                            Duration::from_millis(25),
                            second_ack_client.next(),
                        )
                        .await
                        {
                            Ok(Some(message)) => message,
                            _ => break,
                        };

                        data.push(message);
                    }

                    println!(
                        "ACK (2) - {} ({})",
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
                    "TIME (2): {}",
                    SystemTime::now().duration_since(start).unwrap().as_millis()
                );
                println!("TOTAL (2): {}", message_count);
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
