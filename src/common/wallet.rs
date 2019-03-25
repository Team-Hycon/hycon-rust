use std::error::Error;

use crate::common::signed_tx::SignedTx;
use crate::common::tx::Tx;
use crate::traits::Encode;
use crate::util::hash::hash;

use rand::{Rng, thread_rng};
use secp256k1::key::{PublicKey, SecretKey};
use secp256k1::{Message, RecoverableSignature, Secp256k1};

use bitcoin::network::constants::Network;
use bitcoin::util::bip32::{ChildNumber, ExtendedPrivKey};
use std::str::FromStr;
use wallet::keyfactory::{KeyFactory, Seed};
use wallet::mnemonic::Mnemonic;

type WalletResult<T> = Result<T, Box<Error>>;

pub struct Wallet {
    private_key: SecretKey,
    pub public_key: PublicKey,
}

impl Wallet {
    pub fn new() -> Wallet {
        let secp = Secp256k1::without_caps();
        let mut secret_key = [0u8; 32];
        loop {
            thread_rng().fill(&mut secret_key);
            let priv_key = SecretKey::from_slice(&secp, &secret_key[..]);
            match priv_key {
                Ok(private_key) => {
                    let wallet = Wallet::from_private_key(private_key);
                    return wallet;
                }
                Err(_) => {}
            }
        }
    }

    pub fn get_random_phrase(lang: &str, word_length: usize) -> WalletResult<String> {
        let phrase = Mnemonic::get_random_phrase(lang, word_length)?;
        Ok(phrase)
    }

    pub fn get_ext_key_from_str(ext_key: &str) -> WalletResult<ExtendedPrivKey> {
        Ok(ExtendedPrivKey::from_str(ext_key)?)
    }

    pub fn get_ext_key_from_phrase(
        phrase: &str,
        passphrase: &str,
        lang: &str,
    ) -> WalletResult<ExtendedPrivKey> {
        let mnemonic = Mnemonic::from_phrase(phrase, lang)?;
        let seed = Seed::new(&mnemonic, passphrase);

        // TODO : network with Hycon.
        let ext_key = KeyFactory::master_private_key(Network::Bitcoin, &seed)?;
        return Ok(ext_key);
    }

    pub fn get_wallet_from_ext_key(ext_key: ExtendedPrivKey, index: u32) -> WalletResult<Wallet> {
        let secp = Secp256k1::new();
        let mut chile_numbers: Vec<ChildNumber> = Vec::new();

        // derive path : m/44'/1397'/0'/0/index
        chile_numbers.push(ChildNumber::from_hardened_idx(44));
        chile_numbers.push(ChildNumber::from_hardened_idx(1397));
        chile_numbers.push(ChildNumber::from_hardened_idx(0));
        chile_numbers.push(ChildNumber::from_normal_idx(0));
        chile_numbers.push(ChildNumber::from_normal_idx(index));

        let priv_key = ext_key.derive_priv(&secp, &chile_numbers)?;
        let wallet = Wallet::from_private_key(priv_key.secret_key);
        Ok(wallet)
    }

    pub fn from_private_key(private_key: SecretKey) -> Wallet {
        let secp = Secp256k1::signing_only();
        let public_key = PublicKey::from_secret_key(&secp, &private_key);
        Wallet {
            private_key,
            public_key,
        }
    }

    pub fn sign(&self, message: &Vec<u8>) -> WalletResult<RecoverableSignature> {
        let msg = Message::from_slice(&message)?;
        let secp = Secp256k1::signing_only();
        Ok(secp.sign_recoverable(&msg, &self.private_key))
    }

    pub fn sign_tx(&self, tx: &Tx) -> WalletResult<SignedTx> {
        let encoded_tx = hash(&tx.encode()?, 32);
        let signature = self.sign(&encoded_tx)?;
        let secp = Secp256k1::without_caps();
        let recovery = signature.serialize_compact(&secp).0;

        Ok(SignedTx::from_tx(tx, signature, recovery))
    }

    pub fn generate_private_key<RngType>(rng: &mut RngType) -> SecretKey
    where
        RngType: Rng,
    {
        let secp = Secp256k1::without_caps();
        let mut secret_key = [0u8; 32];
        loop {
            rng.fill(&mut secret_key);
            let priv_key = SecretKey::from_slice(&secp, &secret_key[..]);
            if let Ok(key) = priv_key {
                return key;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::address::{Address, ValidAddress};
    use crate::util::hash::hash;

    use rand::{thread_rng, Rng};
    use secp256k1::{Message, Secp256k1};

    #[test]
    fn it_makes_a_wallet() {
        Wallet::new();
    }

    #[test]
    fn it_signs_data() {
        let wallet = Wallet::new();
        let message = hash("This is a test message to be signed".as_bytes(), 32);
        let secp_message = Message::from_slice(&message[..]).unwrap();
        let recoverable_signature = wallet.sign(&message.to_vec()).unwrap();
        let secp = Secp256k1::verification_only();
        let signature = recoverable_signature.to_standard(&secp);
        let pubkey = secp.recover(&secp_message, &recoverable_signature).unwrap();
        assert_eq!(pubkey, wallet.public_key);
        secp.verify(&secp_message, &signature, &wallet.public_key)
            .unwrap();
    }

    #[test]
    fn it_signs_a_tx() {
        let wallet = Wallet::new();
        let to_wallet = Wallet::new();
        let secp = Secp256k1::verification_only();
        let from = Address::from_pubkey(wallet.public_key);
        let to = Address::from_pubkey(to_wallet.public_key);
        let amount = thread_rng().gen_range(123456, 12345566789);
        let fee = thread_rng().gen_range(1, 12345293847);
        let nonce = thread_rng().gen_range(0, 123456789);
        let tx = Tx::new(from, to, amount, fee, nonce);
        let encoding = tx.encode().unwrap();
        let secp_message = Message::from_slice(&hash(&encoding[..], 32)[..]).unwrap();
        let signed_tx = wallet.sign_tx(&tx).unwrap();

        assert_eq!(signed_tx.from, from);
        assert_eq!(signed_tx.to, to);
        assert_eq!(signed_tx.amount, amount);
        assert_eq!(signed_tx.fee, fee);
        assert_eq!(signed_tx.nonce, nonce);

        let recoverable_signature = signed_tx.signature;
        let signature = recoverable_signature.to_standard(&secp);
        let pubkey = secp.recover(&secp_message, &recoverable_signature).unwrap();
        assert_eq!(pubkey, wallet.public_key);
        secp.verify(&secp_message, &signature, &wallet.public_key)
            .unwrap();
    }

    #[test]
    fn it_return_randomly_generated_mnemonic_depends_on_words_length() {
        let lang = "english";
        let mnemonic12 = Wallet::get_random_phrase(lang, 12).unwrap();
        let mnemonic24 = Wallet::get_random_phrase(lang, 24).unwrap();
        let vec12: Vec<_> = mnemonic12.split(" ").collect();
        let vec24: Vec<_> = mnemonic24.split(" ").collect();
        assert_eq!(vec12.len(), 12);
        assert_eq!(vec24.len(), 24);
    }

    #[test]
    fn it_causes_err_when_recieve_invalid_words_length() {
        assert!(Wallet::get_random_phrase("english", 30).is_err());
    }

    #[test]
    fn it_return_randomly_generated_mnemonic_depends_on_language() {
        let mnemonic_english = Wallet::get_random_phrase("english", 12);
        let mnemonic_korean = Wallet::get_random_phrase("korean", 12);
        let mnemonic_chinese_simplified = Wallet::get_random_phrase("chinese_simplified", 12);
        let mnemonic_chinesesimplified = Wallet::get_random_phrase("chinesesimplified", 12);
        let mnemonic_chinese_traditional = Wallet::get_random_phrase("chinese_traditional", 12);
        let mnemonic_chinesetraditional = Wallet::get_random_phrase("chinesetraditional", 12);
        let mnemonic_japanese = Wallet::get_random_phrase("japanese", 12);
        let mnemonic_french = Wallet::get_random_phrase("french", 12);
        let mnemonic_spanish = Wallet::get_random_phrase("spanish", 12);
        let mnemonic_italian = Wallet::get_random_phrase("italian", 12);

        assert!(mnemonic_english.is_ok());
        assert!(mnemonic_korean.is_ok());
        assert!(mnemonic_chinese_simplified.is_ok());
        assert!(mnemonic_chinesesimplified.is_ok());
        assert!(mnemonic_chinese_traditional.is_ok());
        assert!(mnemonic_chinesetraditional.is_ok());
        assert!(mnemonic_japanese.is_ok());
        assert!(mnemonic_french.is_ok());
        assert!(mnemonic_spanish.is_ok());
        assert!(mnemonic_italian.is_ok());
    }

    #[test]
    fn it_return_root_key_from_mnemonic() {
        let mnemonic_phrase =
            "fiction poem label rigid trick parade crater end car reunion bonus whip";
        let extended_private_key1 =
            Wallet::get_ext_key_from_phrase(mnemonic_phrase, "", "english").unwrap();
        let extended_private_key2 =
            Wallet::get_ext_key_from_phrase(mnemonic_phrase, "passphrase", "english").unwrap();
        let extended_private_key3 =
            Wallet::get_ext_key_from_phrase(mnemonic_phrase, mnemonic_phrase, "english").unwrap();

        assert_eq!(extended_private_key1.to_string(), "xprv9s21ZrQH143K4JHvoqPqrjApSa1Mo1Rj4AoyCCy2neLiXLWd3QoJoAHum6BddvacSaPFaf9eA7cjufyRKV1crEqqVGt9KWVmguSrAkkpUEE");
        assert_eq!(extended_private_key2.to_string(), "xprv9s21ZrQH143K2741favnnve12HKYveu6TiWk99fZvGwacNCoXxZ2aBiZyzPdBLGoyuJB9spnYEpYa58EiBekMfBphuCLHZdByc3my5k6SGY");
        assert_eq!(extended_private_key3.to_string(), "xprv9s21ZrQH143K3n79UN7PunePNPP6ZWbK1EbrvCTLgUKAGDrGn35iD4TnveYp17Lq192Srzmmq9UuPHAdrEpEozHrznm5W7az7f5UyeZmtkx");
    }

    #[test]
    fn it_return_ext_from_string() {
        let ext_key = Wallet::get_ext_key_from_str("xprv9s21ZrQH143K4JHvoqPqrjApSa1Mo1Rj4AoyCCy2neLiXLWd3QoJoAHum6BddvacSaPFaf9eA7cjufyRKV1crEqqVGt9KWVmguSrAkkpUEE").unwrap();

        assert_eq!(ext_key.to_string(), "xprv9s21ZrQH143K4JHvoqPqrjApSa1Mo1Rj4AoyCCy2neLiXLWd3QoJoAHum6BddvacSaPFaf9eA7cjufyRKV1crEqqVGt9KWVmguSrAkkpUEE");
    }

    #[test]
    fn it_return_wallet_from_root_key() {
        let ext_key = Wallet::get_ext_key_from_str("xprv9s21ZrQH143K4JHvoqPqrjApSa1Mo1Rj4AoyCCy2neLiXLWd3QoJoAHum6BddvacSaPFaf9eA7cjufyRKV1crEqqVGt9KWVmguSrAkkpUEE").unwrap();

        let wallet = Wallet::get_wallet_from_ext_key(ext_key, 0).unwrap();
        assert_eq!(
            wallet.private_key.to_string(),
            "702f3280b379cd3b16c816ae66670a716565eaac635dd46e6fe0491dffc97857"
        );
        assert_eq!(
            wallet.public_key.to_string(),
            "03b2cf6433531da53eae8bfd66921a7d2440a46750a02eba365acef273804dbb86"
        );
    }
}
