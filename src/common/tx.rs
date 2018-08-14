use common::{Encode, Proto};
use common::address::{Address, ValidAddress};
use util::hash::hash;
use serialization::tx::Tx as ProtoTx;

use protobuf::{Message as ProtoMessage, ProtobufError};
use secp256k1::{Message, RecoverableSignature, RecoveryId, Secp256k1, Error as SecpError};

#[derive(Debug)]
pub enum EncodingError {
    Proto(ProtobufError),
    Secp(SecpError),
    Integrity(String)
}

#[derive(Clone, Debug, PartialEq)]
pub struct Tx {
    from: Option<Address>,
    to: Option<Address>,
    amount: u64,
    fee: Option<u64>,
    nonce: Option<u32>,
    signature: Option<RecoverableSignature>,
    recovery: Option<RecoveryId>,
}

pub trait Valid<ErrorType> {
    fn verify(&self) -> Result<bool, ErrorType>;
}

impl Tx {
    pub fn new(from: Option<Address>, 
        to: Option<Address>, 
        amount: u64, 
        fee: Option<u64>, 
        nonce: Option<u32>, 
        signature: Option<RecoverableSignature>, 
        recovery: Option<RecoveryId>) -> Tx {
        Tx {
            from,
            to,
            amount,
            fee,
            nonce,
            signature,
            recovery
        }
    }
    pub fn get_amount(&self) -> u64 {
        self.amount
    }
    pub fn get_from(&self) -> Option<Address> {
        self.from
    }
    pub fn get_to(&self) -> Option<Address> {
        self.to
    }
    pub fn get_nonce(&self) -> Option<u32> {
        self.nonce
    }
    pub fn get_fee(&self) -> Option<u64> {
        self.fee
    }
    pub fn get_signature(&self) -> Option<RecoverableSignature> {
        self.signature
    }
    pub fn get_recovery(&self) -> Option<RecoveryId> {
        self.recovery
    }

    pub fn decode(proto_tx: ProtoTx) -> Tx {
        let mut from: Address = [0; 20];
        from.clone_from_slice(&proto_tx.from);
        let mut to: Address = [0; 20];
        to.clone_from_slice(&proto_tx.to);
        let amount = proto_tx.amount;
        let fee = proto_tx.fee;
        let nonce = proto_tx.nonce;
        Tx::new(Some(from), Some(to), amount, Some(fee), Some(nonce), None, None)
    }

    pub fn verify(encoding: Vec<u8>, sender: Address, signature: RecoverableSignature) -> Result<bool, SecpError> {
        let message = Message::from_slice(&hash(&encoding[..], 32))?;
        let secp = Secp256k1::verification_only();
        let pubkey = secp.recover(&message, &signature)?;
        let address = Address::from_pubkey(pubkey);
        if address != sender {
            return Err(SecpError::InvalidSignature);
        }
        let standard_signature = signature.to_standard(&secp);
        match secp.verify(&message, &standard_signature, &pubkey) {
            Ok(_) => return Ok(true),
            Err(e) => return Err(e)
        }
    }

    pub fn equals(&self, other_tx: Tx) -> bool {
        if self.from != other_tx.from {
            false
        } else if self.to != other_tx.to {
            false
        } else if self.amount != other_tx.amount {
            false
        } else if self.fee != other_tx.fee {
            false
        } else if self.nonce != other_tx.nonce {
            false
        } else {
            true
        }
    }
}

impl Proto<EncodingError> for Tx {
    type ProtoType = ProtoTx;
    fn to_proto(&self) -> Result<Self::ProtoType, EncodingError> 
        where Self::ProtoType: ProtoMessage {
        let mut proto_tx = ProtoTx::new();
        match self.get_from() {
            Some(addr) => proto_tx.set_from(addr.to_vec()),
            None => {}
        }
        match self.get_to() {
            Some(addr) => proto_tx.set_to(addr.to_vec()),
            None => {}
        }
        proto_tx.set_amount(self.get_amount());
        match self.get_fee() {
            Some(fee) => proto_tx.set_fee(fee),
            None => {}
        }
        match self.get_nonce() {
            Some(nonce) => proto_tx.set_nonce(nonce),
            None => {}
        }
        Ok(proto_tx)
    }
}

impl Encode<EncodingError> for Tx {
    fn encode(&self) -> Result<Vec<u8>, EncodingError> {
        let proto_tx = self.to_proto()?;
        match proto_tx.write_to_bytes() {
            Ok(data) => Ok(data),
            Err(e) => Err(EncodingError::Proto(e))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn it_makes_a_transaction() {
        let from = [
            230, 104, 95, 253, 219, 134, 92, 215, 230, 126, 105, 213, 18, 95, 30, 166, 128, 229,
            233, 114,
        ];
        let to = [
            87, 217, 90, 40, 10, 141, 125, 74, 177, 128, 155, 18, 148, 149, 135, 84, 9, 224, 232,
            102,
        ];
        let amount = 123456789;
        let fee = 1;
        let nonce = 3;
        let tx = Tx::new(Some(from), Some(to), amount, Some(fee), Some(nonce), None, None);
        assert_eq!(tx.from.unwrap(), from);
        assert_eq!(tx.to.unwrap(), to);
        assert_eq!(tx.amount, amount);
        assert_eq!(tx.fee.unwrap(), fee);
        assert_eq!(tx.nonce.unwrap(), nonce);
        assert_eq!(tx.signature, None);
        assert_eq!(tx.recovery, None);
    }

    #[test]
    fn it_makes_a_transaction_from_itx() {
        let from = [
            230, 104, 95, 253, 219, 134, 92, 215, 230, 126, 105, 213, 18, 95, 30, 166, 128, 229,
            233, 114,
        ];
        let to = [
            87, 217, 90, 40, 10, 141, 125, 74, 177, 128, 155, 18, 148, 149, 135, 84, 9, 224, 232,
            102,
        ];
        let amount = 123456789;
        let fee = 1;
        let nonce = 3;
        let mut itx = ProtoTx::new();
        itx.set_from(from.to_vec());
        itx.set_to(to.to_vec());
        itx.set_amount(amount);
        itx.set_fee(fee);
        itx.set_nonce(nonce);

        let tx = Tx::decode(itx);
        assert_eq!(tx.from.unwrap(), from);
        assert_eq!(tx.to.unwrap(), to);
        assert_eq!(tx.amount, amount);
        assert_eq!(tx.fee.unwrap(), fee);
        assert_eq!(tx.nonce.unwrap(), nonce);
    }

    #[test]
    fn it_encodes_like_javascript_for_non_zero() {
        let from = [
            230, 104, 95, 253, 219, 134, 92, 215, 230, 126, 105, 213, 18, 95, 30, 166, 128, 229,
            233, 114,
        ];
        let to = [
            87, 217, 90, 40, 10, 141, 125, 74, 177, 128, 155, 18, 148, 149, 135, 84, 9, 224, 232,
            102,
        ];
        let amount = 123456789;
        let fee = 1;
        let nonce = 3;
        let tx = Tx::new(Some(from), Some(to), amount, Some(fee), Some(nonce), None, None);
        let encoding = tx.encode().unwrap();
        let expected_encoding = vec![
            10, 20, 230, 104, 95, 253, 219, 134, 92, 215, 230, 126, 105, 213, 18, 95, 30, 166, 128,
            229, 233, 114, 18, 20, 87, 217, 90, 40, 10, 141, 125, 74, 177, 128, 155, 18, 148, 149,
            135, 84, 9, 224, 232, 102, 24, 149, 154, 239, 58, 32, 1, 40, 3,
        ];
        assert_eq!(encoding, expected_encoding);
    }

    #[test]
    fn it_encodes_like_javascript_for_zero() {
        let from: Address =
            Address::from_string(&"H2rCdhQ4fhGk5qX9AwzxA61zhoUKCDVQC".to_string()).unwrap();
        let to: Address =
            Address::from_string(&"Hj3eZJpesfCjrMZfmKXpep6rVWS56Qaz".to_string()).unwrap();
        let amount = 0;
        let fee = 0;
        let nonce = 0;
        let tx = Tx::new(Some(from), Some(to), amount, Some(fee), Some(nonce), None, None);
        let encoding = tx.encode().unwrap();
        let expected_encoding = vec![
            10, 20, 132, 170, 245, 157, 55, 19, 7, 190, 193, 159, 54, 150, 44, 139, 78, 36, 165,
            149, 140, 187, 18, 20, 52, 8, 198, 113, 205, 252, 248, 236, 75, 130, 108, 209, 4, 214,
            46, 51, 111, 17, 216, 146, 24, 0, 32, 0, 40, 0,
        ];
        assert_eq!(encoding, expected_encoding);
    }
}
