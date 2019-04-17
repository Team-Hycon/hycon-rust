use crate::common::address::Address;
use crate::serialization::tx::GenesisTx as ProtoGenesisTx;
use crate::traits::{Decode, Encode, Proto, Transaction};
use std::error::Error;

use protobuf::Message as ProtoMessage;
use secp256k1::{RecoverableSignature, RecoveryId};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GenesisTx {
    to: Address,
    amount: u64,
}

impl Transaction<Address, RecoverableSignature, RecoveryId> for GenesisTx {
    fn get_from(&self) -> Option<Address> {
        None
    }
    fn get_to(&self) -> Option<Address> {
        Some(self.to)
    }
    fn get_amount(&self) -> u64 {
        self.amount
    }
    fn get_fee(&self) -> Option<u64> {
        None
    }
    fn get_nonce(&self) -> Option<u32> {
        None
    }
    fn get_signature(&self) -> Option<RecoverableSignature> {
        None
    }
    fn get_recovery(&self) -> Option<RecoveryId> {
        None
    }
}

impl GenesisTx {
    pub fn new(to: Address, amount: u64) -> GenesisTx {
        GenesisTx { to, amount }
    }
}

impl Proto for GenesisTx {
    type ProtoType = ProtoGenesisTx;
    fn to_proto(&self) -> Result<Self::ProtoType, Box<Error>> {
        let mut proto_genesis_tx = Self::ProtoType::new();
        proto_genesis_tx.set_to(self.to.to_vec());
        proto_genesis_tx.set_amount(self.amount);
        Ok(proto_genesis_tx)
    }
    fn from_proto(_prototype: &Self::ProtoType) -> Result<Self, Box<Error>> {
        unimplemented!()
    }
}

impl Encode for GenesisTx {
    fn encode(&self) -> Result<Vec<u8>, Box<Error>> {
        let proto_tx = self.to_proto()?;
        Ok(proto_tx.write_to_bytes()?)
    }
}

impl Decode for GenesisTx {
    fn decode(buffer: &[u8]) -> Result<GenesisTx, Box<Error>> {
        let mut proto_genesis_tx = ProtoGenesisTx::new();
        proto_genesis_tx.merge_from_bytes(&buffer)?;
        let mut to: Address = [0; 20];
        to.clone_from_slice(&proto_genesis_tx.to);
        Ok(GenesisTx::new(to, proto_genesis_tx.amount))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::prelude::*;
    use rand::{Rng, SeedableRng};

    #[test]
    fn it_makes_a_genesis_transaction() {
        let to = [
            87, 217, 90, 40, 10, 141, 125, 74, 177, 128, 155, 18, 148, 149, 135, 84, 9, 224, 232,
            102,
        ];
        let amount = 123456789;
        let genesis_tx = GenesisTx::new(to, amount);
        assert_eq!(genesis_tx.get_to(), Some(to));
        assert_eq!(genesis_tx.get_amount(), amount);
    }

    #[test]
    fn it_encodes_like_javascript_for_non_zero() {
        let to = [
            87, 217, 90, 40, 10, 141, 125, 74, 177, 128, 155, 18, 148, 149, 135, 84, 9, 224, 232,
            102,
        ];
        let amount = 123456789;
        let genesis_tx = GenesisTx::new(to, amount);
        let encoding = genesis_tx.encode().unwrap();
        let expected_encoding = vec![
            18, 20, 87, 217, 90, 40, 10, 141, 125, 74, 177, 128, 155, 18, 148, 149, 135, 84, 9,
            224, 232, 102, 24, 149, 154, 239, 58,
        ];
        assert_eq!(encoding, expected_encoding);
    }

    #[test]
    fn it_encodes_like_javascript_for_zero() {
        let to = [
            87, 217, 90, 40, 10, 141, 125, 74, 177, 128, 155, 18, 148, 149, 135, 84, 9, 224, 232,
            102,
        ];
        let amount = 0;
        let genesis_tx = GenesisTx::new(to, amount);
        let encoding = genesis_tx.encode().unwrap();
        let expected_encoding = vec![
            18, 20, 87, 217, 90, 40, 10, 141, 125, 74, 177, 128, 155, 18, 148, 149, 135, 84, 9,
            224, 232, 102, 24, 0,
        ];
        assert_eq!(encoding, expected_encoding);
    }

    #[test]
    fn it_decodes_a_genesis_tx() {
        let to = [
            87, 217, 90, 40, 10, 141, 125, 74, 177, 128, 155, 18, 148, 149, 135, 84, 9, 224, 232,
            102,
        ];
        let amount = 123456789;
        let genesis_tx = GenesisTx::new(to, amount);
        let encoding = genesis_tx.encode().unwrap();
        let decoded_genesis_tx = GenesisTx::decode(&encoding).unwrap();
        assert_eq!(genesis_tx, decoded_genesis_tx);
    }

    #[test]
    #[should_panic]
    fn it_fails_to_decode_random_bad_bytes() {
        let seed = [0x77u8; 32];
        let mut rng: StdRng = SeedableRng::from_seed(seed);
        let mut random_bytes = [0u8; 256];
        rng.fill(&mut random_bytes);
        GenesisTx::decode(&random_bytes.to_vec()).unwrap();
    }
}
