use crate::common::block::Block;
use crate::common::block_status::BlockStatus;
use crate::common::header::{BlockHeader, Header};
use crate::common::meta::Meta;
use crate::common::signed_tx::SignedTx;
use crate::consensus::difficulty_adjuster;
use crate::consensus::state_processor::StateProcessor;
use crate::consensus::BlockForkChoice;
use crate::database::block_db::BlockDB;
use crate::traits::{Encode, Exception};
use crate::util::hash::{hash, hash_cryptonight};

use std::cmp::Ordering;
use std::error::Error;
use std::sync::{Arc, Mutex};

type PutResult<T> = Result<T, Box<Error>>;

impl BlockForkChoice for Meta {
    fn fork_choice(&self, other: &Meta) -> Ordering {
        self.total_work.partial_cmp(&other.total_work).unwrap()
    }
}
pub struct Consensus {
    tip_height: usize,
    block_db: Arc<Mutex<BlockDB>>,
    state_processor: StateProcessor,
}

impl Consensus {
    pub fn new(
        state_processor: StateProcessor,
        block_db: Arc<Mutex<BlockDB>>,
    ) -> Result<Self, Box<Error>> {
        Ok(Self {
            block_db,
            state_processor,
            tip_height: 0,
        })
    }
}

impl HyconConsensus<Header, Block<Header, SignedTx>> for Consensus {
    fn init(&mut self) -> Result<(), Box<Error>> {
        if let Some(tip_height) = self.get_tip_height() {
            self.tip_height = tip_height
        } else {
            // init exodus
        }

        Ok(())
    }
    fn get_tip_height(&self) -> Option<usize> {
        None
    }

    fn put(&mut self, header: Header, block: Option<Block<Header, SignedTx>>) -> PutResult<()> {
        Ok(())
    }
}

impl HeaderProcessor<Header> for Consensus {
    type UncleProcessResult = UncleResult;
    fn process_header(&self, header: &Header) -> Result<(), Box<Error>> {
        if header.previous_hash.len() == 0 {
            return Err(Box::new(Exception::new("Block Rejected: No previous hash")));
        }

        match self
            .block_db
            .lock()
            .map_err(|_e| Exception::new("Poison Error"))?
            .get_block_status(&header.previous_hash[0])?
        {
            BlockStatus::Rejected => {
                return Err(Box::new(Exception::new(
                    "Block Ignored: Previous block rejected",
                )));
            }
            _ => {}
        }
        let mut prehash = header.prehash()?;
        prehash.append(&mut header.nonce.to_le_bytes().to_vec());
        if difficulty_adjuster::acceptable(
            hash_cryptonight(&prehash, prehash.len()),
            difficulty_adjuster::get_target(header.difficulty, 32)?,
        )? {
            self.block_db
                .lock()
                .map_err(|_e| Exception::new("Poison Error"))?
                .set_block_status(&hash(&header.encode()?, 32), BlockStatus::Header)?;
            Ok(())
        } else {
            self.block_db
                .lock()
                .map_err(|_e| Exception::new("Poison Error"))?
                .set_block_status(&hash(&header.encode()?, 32), BlockStatus::Rejected)?;
            Err(Box::new(Exception::new(
                "Block rejected, hash does not match difficulty",
            )))
        }
    }

    fn process_uncles(&self, uncle_hashes: &[Vec<u8>]) -> Result<UncleResult, Box<Error>> {
        for hash in uncle_hashes {
            match self
                .block_db
                .lock()
                .map_err(|_e| Exception::new("Poison Error"))?
                .get_block_status(hash)
            {
                _ => {}
            }
        }
        Ok(UncleResult::Success)
    }
}

pub enum UncleResult {
    Success,
    Partial(Vec<Vec<u8>>),
    Failure(Vec<Vec<u8>>),
}

impl ForkChoice<Meta> for Consensus {
    fn fork_choice(tip: &Meta, new_block: &Meta) -> bool {
        match new_block.fork_choice(tip) {
            Ordering::Greater => true,
            _ => false,
        }
    }
}
/// # ForkChoice Trait
/// Defines how the blockchain behaves in the event of a forking event
/// ___
/// ## Type Constraints
///
/// ### BlockType
/// Must implement the BlockForkChoice trait
/// ___
/// ## Methods
/// ### Fork Choice
/// ```
/// fn fork_choice(tip: &BlockType, &new_block: &BlockType) -> bool;
/// ```
/// #### Arguments
/// - `tip` - the current tip of the blockchain
/// - `new_block` - the new block that should be added to the existing tip
///
/// #### Returns
/// - `true` - if the block can be added to the chain
/// - `false` - if the block should not extend the existing chain
/// ___
pub trait ForkChoice<BlockType>
where
    BlockType: BlockForkChoice,
{
    fn fork_choice(tip: &BlockType, new_block: &BlockType) -> bool;
}

/// # Header Processor Trait
/// Defines methods to be used in the processing of blockchain headers
/// ___
/// ## Type Constraints
///
///  ### HeaderType
/// Must implement the BlockHeader trait
/// ### UncleProcessType
/// User defined type to contain the result of processing uncle blocks
/// ___
/// ## Required Methods
/// ### Process Header
/// ```
/// fn process_header(&self, header: &HeaderType) -> Result<(), Box<std::error::Error>>
/// ```
/// Defines how a header is processed
/// #### Arguments
/// `header` - a reference to the HeaderType being processed
///
/// #### Returns
/// ```
/// Result<(), Box<std::error::Error>>
/// ```
/// An empty result denoting success
/// ___
/// ### Process Uncles
/// ```
/// fn process_uncles(&self,uncle_hashes: &[Vec<u8>]) -> Result<Self::UncleProcessResult,Box<std::error::Error>>
/// ```
/// #### Arguments
/// `uncle_hashes` - a slice of block hashes to be checked for validity as uncle blocks
///
/// #### Returns
/// ```
/// Result<Self::UncleProcessResult,Box<std::error::Error>>
/// ```
/// A result containing the user defined UncleProcessResult type
///___
pub trait HeaderProcessor<HeaderType>
where
    HeaderType: BlockHeader,
{
    type UncleProcessResult;
    fn process_header(&self, header: &HeaderType) -> Result<(), Box<Error>>;
    fn process_uncles(
        &self,
        uncle_hashes: &[Vec<u8>],
    ) -> Result<Self::UncleProcessResult, Box<Error>>;
}

pub trait BlockProcessor<BlockType, HeaderType, TxType, MetaType, SignatureType>
where
    MetaType: BlockForkChoice,
{
    fn split_block(
        &self,
        block: &BlockType,
    ) -> (HeaderType, Vec<TxType>, MetaType, Vec<SignatureType>);
}

pub trait StateProcessorTrait<TxType> {
    fn process_txs(&self, txs: &[TxType]) -> Result<(), Box<Error>>;
}

pub trait TxProcessor<TxType> {
    fn check_signatures(&self, txs: &[TxType]) -> Result<(), Box<Error>>;
}

pub trait HyconConsensus<HeaderType, BlockType> {
    fn init(&mut self) -> Result<(), Box<Error>>;
    fn get_tip_height(&self) -> Option<usize>;
    fn put(&mut self, header: HeaderType, block: Option<BlockType>) -> PutResult<()>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::address::{Address, ValidAddress};
    use crate::common::block_status::BlockStatus;
    use crate::consensus::worldstate::WorldState;
    use crate::database::block_db::BlockDB;
    use crate::database::dbkeys::DBKeys;
    use crate::database::state_db::StateDB;

    use rust_base58::FromBase58;
    use std::path::PathBuf;
    #[test]
    fn it_assigns_nonce_bytes_correctly() {
        let nonce: u64 = 9991999136134178034;
        let le_nonce = nonce.to_le_bytes();
        let expected_bytes = [242, 164, 73, 65, 70, 182, 170, 138];
        assert_eq!(le_nonce, expected_bytes);
    }

    #[test]
    fn it_correctly_checks_cn_hash_of_header() {
        //Set up consensus
        let state_path = PathBuf::from("state");
        let block_path = PathBuf::from("blocks");
        let file_path = PathBuf::from("blockfile");
        let keys = DBKeys::default();
        let block_db = BlockDB::new(block_path, file_path, keys, None).unwrap();
        let db_wrapper = Arc::new(Mutex::new(block_db));
        let state_db = StateDB::new(state_path, None).unwrap();
        let world_state = WorldState::new(state_db, 20).unwrap();
        let state_processor = StateProcessor::new(db_wrapper.clone(), world_state);
        let consensus = Consensus::new(state_processor, db_wrapper).unwrap();

        // Set up header - Block Number 864589
        let merkle_root = "xyw95Bsby3s4mt6f4FmFDnFVpQBAeJxBFNGzu2cX4dM"
            .from_base58()
            .unwrap();
        let state_root = "2TQHHSG8daQxYMCisMywUppz7a3YXb83VKzpMsZwQgLi"
            .from_base58()
            .unwrap();
        let time_stamp = 1553674416492;
        let difficulty = 0.00000004430078803796533 as f64;
        let nonce = 12610086967913974370;
        let miner = Address::from_string(&"H2zF9ZrneniGejpGSs7dpafiU7vJACFTW".to_string()).unwrap();
        let previous_hash = vec!["3u3jNwUbeMPiRBMCy2hHLRtGmiYQJmxMwyv1aUGV7AMN"
            .from_base58()
            .unwrap()];

        let header = Header::new(
            merkle_root.clone(),
            time_stamp,
            difficulty,
            state_root.clone(),
            previous_hash.clone(),
            nonce,
            miner,
        );
        let res = consensus.process_header(&header);
        assert!(res.is_ok());
    }

    #[test]
    fn it_chooses_the_correct_fork() {
        let height = 123456789;
        let t_ema = 1234.0;
        let p_ema = 0.1234;
        let next_difficulty = 0.012345;
        let total_work = 1e23;
        let offset = 123;
        let file_number = 234;
        let length = 345;
        let tip_meta = Meta::new(
            height,
            t_ema,
            p_ema,
            next_difficulty,
            total_work,
            Some(file_number),
            Some(offset),
            Some(length),
            BlockStatus::Header,
        );
        let second_tw = 1.000000000001e23;
        let new_meta = Meta::new(
            height,
            t_ema,
            p_ema,
            next_difficulty,
            second_tw,
            Some(file_number),
            Some(offset),
            Some(length),
            BlockStatus::Invalid,
        );
        assert!(Consensus::fork_choice(&tip_meta, &new_meta));
        assert_ne!(Consensus::fork_choice(&new_meta, &tip_meta), true);
    }
}
