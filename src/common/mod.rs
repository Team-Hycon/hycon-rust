pub mod address;
pub mod block;
pub mod block_status;
pub mod exodus_block;
pub mod exodus_tx;
pub mod genesis_block;
pub mod genesis_header;
pub mod genesis_tx;
pub mod header;
pub mod key_store;
pub mod merkle;
pub mod meta;
pub mod signed_genesis_tx;
pub mod signed_tx;
pub mod test_functions;
pub mod transaction;
pub mod tx;
pub mod tx_pool;
pub mod wallet;

// use std::error::Error;
// use std::fmt::{Display, Formatter, Result as FmtResult};
// use std::marker::Sized;
// use std::result::Result;
// pub trait Encode {
//     fn encode(&self) -> Result<Vec<u8>, Box<Error>>;
// }

// pub trait Decode {
//     type ProtoType;
//     fn decode(buffer: &Vec<u8>) -> Result<Self, Box<Error>>
//     where
//         Self: Sized;
// }

// pub trait Proto {
//     type ProtoType;
//     fn to_proto(&self) -> Result<Self::ProtoType, Box<Error>>;
// }

// #[derive(Debug)]
// pub struct Exception {
//     details: String,
// }

// impl Exception {
//     pub fn new(details: &str) -> Exception {
//         Exception {
//             details: details.to_string(),
//         }
//     }
// }

// impl Display for Exception {
//     fn fmt(&self, f: &mut Formatter) -> FmtResult {
//         write!(f, "{}", self.details)
//     }
// }

// impl Error for Exception {
//     fn description(&self) -> &str {
//         &self.details
//     }
// }
