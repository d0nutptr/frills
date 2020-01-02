use crate::server::message::NewConnectionNotification;
use crate::server::ClientThread;
use slab::Slab;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc::{channel, Receiver, Sender};

pub struct ClientConnectListener {
    listener: TcpListener,
    master_channel: Sender<NewConnectionNotification>,
}

impl ClientConnectListener {
    pub async fn new(port: u16, master_channel: Sender<NewConnectionNotification>) -> Self {
        let listener = TcpListener::bind(SocketAddr::new(IpAddr::from([0, 0, 0, 0]), port))
            .await
            .unwrap();

        Self {
            listener,
            master_channel,
        }
    }

    pub async fn listen(mut self) {
        loop {
            match self.listener.accept().await {
                Ok((_socket, addr)) => {
                    self.master_channel
                        .send(NewConnectionNotification { stream: _socket })
                        .await;
                }
                Err(e) => println!("couldn't get client: {:?}", e),
            }
        }
    }
}
