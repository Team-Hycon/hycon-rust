use crate::serialization::network::{Network as ProtoNetwork, Network_oneof_request};
use crate::server::{Decode, Encode, Exception, Proto};
use bytes::BytesMut;
use protobuf::{CodedInputStream, Message as ProtoMessage};
use std::error::Error;
pub struct NetworkMessage {
    pub message_type: Network_oneof_request,
}

impl NetworkMessage {
    pub fn new(message_type: Network_oneof_request) -> Self {
        Self { message_type }
    }
}

impl Decode for NetworkMessage {
    type ProtoType = ProtoNetwork;
    fn decode(buffer: &Vec<u8>) -> Result<Self, Box<Error>> {
        let mut message: ProtoNetwork = ProtoNetwork::new();
        if let Err(_) = message.merge_from(&mut CodedInputStream::from_bytes(buffer.as_slice())) {
            // return Err(Box::new(Exception::new("Decoding fail")));
        }
        if let Some(message_type) = message.request {
            return Ok(Self { message_type });
        }
        Ok(Self {
            message_type: Network_oneof_request::statusReturn(
                crate::serialization::network::StatusReturn::new(),
            ),
        })
    }
}

impl Encode for NetworkMessage {
    fn encode(&self) -> Result<Vec<u8>, Box<Error>> {
        let proto_message = self.to_proto()?;
        Ok(proto_message.write_to_bytes()?)
    }
}

impl Proto for NetworkMessage {
    type ProtoType = ProtoNetwork;
    fn to_proto(&self) -> Result<Self::ProtoType, Box<Error>> {
        let mut proto_message = Self::ProtoType::new();
        match self.message_type.clone() {
            Network_oneof_request::status(v) => {
                proto_message.set_status(v);
            }
            Network_oneof_request::statusReturn(v) => {
                proto_message.set_statusReturn(v);
            }
            _ => {}
        }
        Ok(proto_message)
    }
}
#[derive(Clone)]
pub struct NetworkManager {}

impl NetworkManager {
    pub fn decode(bytes: &Vec<u8>) -> Result<NetworkMessage, Box<Error>> {
        NetworkMessage::decode(bytes)
    }
}