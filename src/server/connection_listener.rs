use crate::server::message::NewConnectionNotification;
use std::net::{IpAddr, SocketAddr};
use tokio::net::TcpListener;
use tokio::sync::mpsc::Sender;

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

    pub async fn listen(self) {
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
