pub mod address;
pub mod genesis_tx;
pub mod genesis_signed_tx;
pub mod signed_tx;
pub mod tx;
pub mod header;
pub mod genesis_header;
pub mod block;
pub mod genesis_block;
pub mod meta_info;

pub trait Encode {
    fn encode(&self) -> Result<Vec<u8>, String>;
}