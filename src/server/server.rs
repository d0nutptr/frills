use sharded_slab::Slab;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use futures_util::stream::StreamExt;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc::{Sender, Receiver, channel};
use tokio_util::codec::{Encoder, Decoder, Framed};
use std::net::{SocketAddr, IpAddr, SocketAddrV4, Ipv4Addr};
use std::future::Future;
use std::task::{Context, Poll};
use tokio::prelude::*;
use bytes::{BytesMut, BufMut};
use futures::io::Error;
use futures_util::pin_mut;
use futures_core::stream::Stream;
use futures::future::{select, Either};
use std::pin::Pin;
use pin_project::pin_project;

pub struct ClientListener {
    listener: TcpListener,
    clients: Slab<Sender<ClientAndMasterConversation>>
}

impl ClientListener {
    pub async fn new(port: u16) -> Self {
        let listener = TcpListener::bind(SocketAddr::new(IpAddr::from([0, 0, 0, 0]), port)).await.unwrap();

        Self {
            listener,
            clients: Slab::new()
        }
    }

    pub async fn listen(mut self) {
        loop {
            match self.listener.accept().await {
                Ok((_socket, addr)) => {
                    let (sender, receiver) = channel(1);

                    let client = Client::new(_socket, sender, receiver);

                    tokio::spawn(client);
                },
                Err(e) => println!("couldn't get client: {:?}", e),
            }
        }
    }
}

struct FrillsCodec {}

struct FrillsCodecError {}

impl From<std::io::Error> for FrillsCodecError {
    fn from(_: Error) -> Self {
        FrillsCodecError{}
    }
}

impl Encoder for FrillsCodec {
    type Item = FrillsMessage;
    type Error = FrillsCodecError;

    fn encode(&mut self, item: Self::Item, dst: &mut BytesMut) -> Result<(), Self::Error> {
        match bincode::serialize(&item) {
            Ok(output) => {
                dst.reserve(output.len());
                dst.put_slice(&output);
                Ok(())
            },
            _ => Err(FrillsCodecError{})
        }
    }
}

impl Decoder for FrillsCodec {
    type Item = FrillsMessage;
    type Error = FrillsCodecError;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        if src.is_empty() {
            return Ok(None);
        }

        match bincode::deserialize::<FrillsMessage>(src) {
            Ok(output) => {
                // we need to know how many bytes to slice off now
                let len = bincode::serialize(&output).unwrap().len();

                src.split_to(len);

                Ok(Some(output))
            },
            Err(e) => Ok(None)
        }
    }
}

#[pin_project]
pub struct Client {
    #[pin]
    stream: Framed<TcpStream, FrillsCodec>,// TcpStream
    sender: Sender<ClientAndMasterConversation>,
    #[pin]
    receiver: Receiver<ClientAndMasterConversation>
}

impl Client {
    pub fn new(stream: TcpStream, sender: Sender<ClientAndMasterConversation>, receiver: Receiver<ClientAndMasterConversation>) -> Self {
        Self {
            stream: Framed::new(stream, FrillsCodec{}),
            sender,
            receiver
        }
    }
}

impl Future for Client {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut projected = self.project();

        match projected.stream.poll_next(cx) {
            Poll::Ready(Some(item)) => {
                cx.waker().wake_by_ref();
                // do a thing
                println!("RECEIVED A MESSAGE FROM TCP STREAM")
            },
            _ => {}
        };

        match projected.receiver.poll_recv(cx) {
            Poll::Ready(Some(item)) => {
                cx.waker().wake_by_ref();
                println!("RECEIVED A MESSAGE FROM MASTER")
            },
            _ => {}
        };

        Poll::Pending
    }
}

#[derive(Deserialize, Serialize)]
pub enum FrillsMessage {
    Empty
}

pub enum ClientAndMasterConversation {
    ClientToListener(ClientToMasterMessage),
    MasterToClient(MasterToClientMessage)
}

pub enum ClientToMasterMessage {
    NewMessage {
        topic: u32,
        message: Vec<u32>
    },
    Ack {
        client_id: u32,
        message_id: u32
    },
    Nack {
        client_id: u32,
        message_id: u32
    },
    Disconnect {
        client_id: u32
    }
}

pub enum MasterToClientMessage {
    NewMessage {
        message_id: u32,
        message: Vec<u32>,
    }
}

