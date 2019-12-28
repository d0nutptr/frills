use serde::{Deserialize, Serialize};
use crate::server::ClientListener;
use tokio::net::TcpStream;
use tokio::prelude::*;
use std::net::{SocketAddr, SocketAddrV4, Ipv4Addr};

#[test]
fn bincode() {
    let data = ExampleNestedStructure {
        version: 123,
        value: InternalValue {
            value: r#"{"msg": "Some Fake Data"}"#.to_string()
        }
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
fn test_tcp_connection() {
    let mut runtime = tokio::runtime::Runtime::new().unwrap();

    runtime.block_on(async {
        let mut listener = ClientListener::new(12345).await;

        tokio::spawn(async {
            let test_data = crate::server::FrillsMessage::Empty{ };

            let mut stream = TcpStream::connect(SocketAddrV4::new(Ipv4Addr::new(0, 0, 0, 0), 12345)).await.unwrap();

            println!("Connected to test bench.");

            for i in 0 .. 100 {
                stream.write_all(&bincode::serialize(&test_data).unwrap()).await;
            }

            println!("Data wrote!");
        });

        listener.listen().await;
    });
}

#[derive(Serialize, Deserialize)]
struct ExampleNestedStructure {
    pub version: u32,
    pub value: InternalValue
}

#[derive(Serialize, Deserialize)]
struct InternalValue {
    pub value: String
}