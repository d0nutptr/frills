use crate::codec::FrillsMessage;
use bytes::{BufMut, BytesMut};
use std::fmt::{Debug, Formatter};
use tokio_util::codec::{Decoder, Encoder};

pub struct FrillsCodec {}

pub struct FrillsCodecError {}

impl Debug for FrillsCodecError {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), std::fmt::Error> {
        Ok(())
    }
}

impl From<std::io::Error> for FrillsCodecError {
    fn from(_: std::io::Error) -> Self {
        FrillsCodecError {}
    }
}

impl Encoder<FrillsMessage> for FrillsCodec {
    type Error = FrillsCodecError;

    fn encode(&mut self, item: FrillsMessage, dst: &mut BytesMut) -> Result<(), Self::Error> {
        match bincode::serialize(&item) {
            Ok(output) => {
                dst.reserve(output.len());
                dst.put_slice(&output);
                Ok(())
            }
            _ => Err(FrillsCodecError {}),
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
            }
            Err(e) => Ok(None),
        }
    }
}
