use std::error::Error;
use std::path::PathBuf;

use crate::database::IDB;
use crate::serialization::state::ProtoMerkleNode;
use crate::traits::{Decode, Encode, Exception};

use rocksdb::{
    BlockBasedIndexType, BlockBasedOptions, Options as RocksDBOptions, SliceTransform, WriteBatch,
    DB as RocksDB,
};

use starling::traits::Database as IDatabase;

impl IDatabase for StateDB {
    type NodeType = ProtoMerkleNode;
    type EntryType = (Vec<u8>, Self::NodeType);

    fn open(_path: &PathBuf) -> Result<StateDB, Box<Error>> {
        return Err(Box::new(Exception::new(
            "Open the database using new, not open",
        )));
    }

    fn get_node(&self, key: &[u8]) -> Result<Option<Self::NodeType>, Box<Error>> {
        let bytes = self.database._get(key)?;
        Ok(Some(Self::NodeType::decode(&bytes)?))
    }

    fn insert(&mut self, key: &[u8], value: &Self::NodeType) -> Result<(), Box<Error>> {
        self.pending_inserts.push((key.to_vec(), value.clone()));
        Ok(())
    }

    fn remove(&mut self, key: &[u8]) -> Result<(), Box<Error>> {
        self.database.delete(key)?;
        Ok(())
    }

    fn batch_write(&mut self) -> Result<(), Box<Error>> {
        let mut batch = WriteBatch::default();
        while self.pending_inserts.len() > 0 {
            let entry = self.pending_inserts.remove(0);
            let key = entry.0;
            let value = entry.1;
            batch.put(key.as_ref(), value.encode()?.as_ref())?;
        }
        self.database.write(batch)?;
        Ok(())
    }
}

pub struct StateDB<DatabaseType = RocksDB, EntryType = (Vec<u8>, ProtoMerkleNode)> {
    database: DatabaseType,
    pending_inserts: Vec<EntryType>,
}

impl<DatabaseType, EntryType, OptionType> StateDB<DatabaseType, EntryType>
where
    DatabaseType: IDB<OptionType = OptionType>,
{
    pub fn new(
        path: PathBuf,
        options: Option<OptionType>,
    ) -> Result<StateDB<DatabaseType, EntryType>, Box<Error>> {
        let database = DatabaseType::open(path, options)?;
        let pending_inserts = Vec::with_capacity(8193);
        Ok(StateDB {
            database,
            pending_inserts,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::mock::RocksDBMock;
    use std::collections::HashMap;

    use crate::serialization::state::Data as ProtoData;

    #[test]
    fn it_opens_a_state_db() {
        let path = PathBuf::new();
        let state_db: StateDB<RocksDBMock> = StateDB::new(path, None).unwrap();
    }
}