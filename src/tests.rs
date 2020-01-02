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
use tokio::runtime::Runtime;
use serde::de::value::StringDeserializer;

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

            let mut ack_client = FrillsClient::builder("AlwaysACK")
                .remote_from_str("127.0.0.1:12345")
                .cache_size(25000)
                .build().await
                .unwrap();

            let mut producer_handle = producer_client.get_client_handle();
            let mut ack_client_handle = ack_client.get_client_handle();

            let topic = "DataStream";

            producer_handle.register_topic(topic).await;
            ack_client_handle.subscribe_to_topic(topic).await;

            // wait a bit to ensure topic was registered
            tokio::time::delay_for(Duration::from_millis(250)).await;

            // push some messages into queue
            let messages: Vec<Vec<u8>> = (0 .. 1_000_000u32).map(|i| format!("{}", i).into_bytes()).collect();

            producer_handle
                .push_messages(topic, messages)
                .await;

            // Waiting for the messages to be pushed up
            tokio::time::delay_for(Duration::from_millis(2000)).await;

            println!("Starting fetch..");

            tokio::spawn(async move {
                let start = SystemTime::now();

                run_ack_client("AlwaysACK", 8192, "127.0.0.1:12345", topic.clone()).await;

                println!("TIME(1): {}", SystemTime::now().duration_since(start).unwrap().as_millis());
            });

            tokio::spawn(async move {
                let start = SystemTime::now();

                run_ack_client("AlwaysACK", 8192, "127.0.0.1:12345", topic.clone()).await;

                println!("TIME(2): {}", SystemTime::now().duration_since(start).unwrap().as_millis());
            });

            tokio::spawn(async move {
                let start = SystemTime::now();

                run_ack_client("AlwaysACK", 8192, "127.0.0.1:12345", topic.clone()).await;

                println!("TIME(3): {}", SystemTime::now().duration_since(start).unwrap().as_millis());
            });

            tokio::spawn(async move {
                let start = SystemTime::now();

                run_ack_client("AlwaysACK", 8192, "127.0.0.1:12345", topic.clone()).await;

                println!("TIME(4): {}", SystemTime::now().duration_since(start).unwrap().as_millis());
            });
        });

        // start the server :)
        server.run().await;
    });
}

async fn run_ack_client(service_name: &str, cache_size: u16, remote: &str, topic: &str) {
    let mut ack_client = FrillsClient::builder(service_name)
        .cache_size(cache_size)
        .remote_from_str(remote)
        .build().await
        .unwrap();

    let mut ack_client_handle = ack_client.get_client_handle();
    ack_client_handle.subscribe_to_topic(topic).await;

    let mut message_count = 0u32;

    loop {
        let mut data = Vec::new();

        for _ in 0..10_000u32 {
            let message = match tokio::time::timeout(
                Duration::from_millis(250),
                ack_client.next(),
            ).await {
                Ok(Some(message)) => message,
                _ => break,
            };

            data.push(message);
        }

        ack_client_handle.ack_message(data.iter().map(|message| message.message_id).collect()).await;

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

        if data.len() < 10_000 { break }
    }
    println!("TOTAL: {}", message_count);
}

#[test]
fn puppy_test() {
    let mut runtime = Runtime::new().unwrap();;

    runtime.block_on(async {
        let mut server = FrillsServer::new(12345);

        tokio::spawn(async {
            // give the server some time to start
            tokio::time::delay_for(Duration::from_millis(250)).await;

            let mut dog_client = FrillsClient::builder("DogClient")
                .remote_from_str("127.0.0.1:12345")
                .cache_size(1)
                .build().await
                .unwrap();
            let mut dog_handle = dog_client.get_client_handle();

            dog_handle.register_topic("DogNames").await;
            dog_handle.register_topic("l33tPuppyNames").await;
            dog_handle.subscribe_to_topic("l33tPuppyNames").await;

            // to ensure all of the topics are registered properly
            tokio::time::delay_for(Duration::from_millis(250)).await;

            // let's push out some dog names
            let dog_names = vec!["fido", "rufus", "taiko"];
            dog_handle.push_messages("DogNames", dog_names.into_iter().map(|name| name.as_bytes().to_vec()).collect()).await;

            // damned async closures don't work yet; gotta use `while let`
            // for more speed, convert this to be concurrent by pulling additional messages
            // and executing your tasks in parallel with join_all
            while let Some(message) = dog_client.next().await {
                let puppy_name = String::from_utf8(message.message).unwrap();
                let dog_name = puppy_to_dog(puppy_name.clone());

                println!("(Dog Client): Converted {} to {}", puppy_name, dog_name);

                dog_handle.push_messages("DogNames", vec![dog_name.into_bytes().to_vec()]).await
            }
        });

        tokio::spawn(async {
            // give the server some time to start
            tokio::time::delay_for(Duration::from_millis(250)).await;

            let mut puppy_client = FrillsClient::builder("l33tPuppyClient")
                .remote_from_str("127.0.0.1:12345")
                .cache_size(1)
                .build().await
                .unwrap();
            let mut puppy_handle = puppy_client.get_client_handle();

            puppy_handle.register_topic("DogNames").await;
            puppy_handle.register_topic("l33tPuppyNames").await;
            puppy_handle.subscribe_to_topic("DogNames").await;


            // to ensure all of the topics are registered properly
            tokio::time::delay_for(Duration::from_millis(250)).await;

            // damned async closures don't work yet; gotta use `while let`
            // for more speed, convert this to be concurrent by pulling additional messages
            // and executing your tasks in parallel with join_all
            while let Some(message) = puppy_client.next().await {
                let dog_name = String::from_utf8(message.message).unwrap();
                let puppy_name = dog_to_puppy(dog_name.clone());

                println!("(133t Puppy Client): Converted {} to {}", dog_name, puppy_name);

                puppy_handle.push_messages("l33tPuppyNames", vec![puppy_name.into_bytes().to_vec()]).await
            }
        });

        server.run().await;
    });
}


fn puppy_to_dog(puppy_name: String) -> String {
    puppy_name.replace("1", "i")
        .replace("0", "o")
        .replace("5", "s")
        .replace("@", "a")
}

fn dog_to_puppy(dog_name: String) -> String {
    dog_name.replace("i", "1")
        .replace("o", "0")
        .replace("s", "5")
        .replace("a", "@")
}