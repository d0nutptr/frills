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
use futures::io::Error as IOError;
use futures_util::sink::SinkExt;
use futures_util::{pin_mut, select};
use futures_core::stream::Stream;
use std::pin::Pin;
use pin_project::pin_project;
use std::fmt::Debug;
use serde::export::Formatter;
use futures::future::Either;

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

                    tokio::spawn(client.run());
                },
                Err(e) => println!("couldn't get client: {:?}", e),
            }
        }
    }
}

struct FrillsCodec {}

struct FrillsCodecError {}

impl Debug for FrillsCodecError {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), std::fmt::Error> {
        Ok(())
    }
}

impl From<std::io::Error> for FrillsCodecError {
    fn from(_: std::io::Error) -> Self {
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

    pub async fn run (mut self) {
        let mut client_stream = self.stream;
        let mut master_receiver = self.receiver;

        loop {
            let next = {
                let mut either = next_either(client_stream, master_receiver);
                let next = either.next().await.unwrap();

                // release the streams so we can work with them again (namely, tcp_stream)
                let (released_client_stream, released_master_receiver) = either.release();
                client_stream = released_client_stream;
                master_receiver = released_master_receiver;

                next
            };

            match next {
                // TCP
                Either::Left(item) => {
                    client_stream.send(item.unwrap());
                    println!("Got a message!");
                },
                // MASTER
                Either::Right(item) => {

                }
            };
        }

        // return the streams
        self.stream = client_stream;
        self.receiver = master_receiver;
    }
}

#[pin_project]
struct NextEither<A, B>
where
    A: Stream,
    B: Stream {
    #[pin]
    left: A,
    #[pin]
    right: B
}

impl<A, B> NextEither<A, B>
where
    A: Stream,
    B: Stream {
    fn new(left: A, right: B) -> Self {
        Self {
            left,
            right
        }
    }

    /// Releases the streams
    fn release(self) -> (A, B) {
        (self.left, self.right)
    }
}

impl<A, B> Stream for NextEither<A, B>
where
    A: Stream,
    B: Stream {
    type Item = Either<A::Item, B::Item>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut projection = self.project();

        // todo either require that these are fused (we need to check that framed streams can be
        // fused and then produce values after receiving more bytes
        match projection.left.poll_next(cx) {
            Poll::Ready(Some(output)) => {
                return Poll::Ready(Some(Either::Left(output)))
            },
            _ => {},
        };

        match projection.right.poll_next(cx) {
            Poll::Ready(Some(output)) => {
                return Poll::Ready(Some(Either::Right(output)))
            },
            _ => {},
        };

        Poll::Pending
    }
}

fn next_either<A, B>(left: A, right: B) -> NextEither<A, B>
where
    A: Stream,
    B: Stream {
    NextEither::new(left, right)
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

