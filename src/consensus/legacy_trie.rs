use crate::account::account::Account;
use crate::account::db_state::DBState;
use crate::account::node_ref::NodeRef;
use crate::account::state_node::StateNode;
use crate::common::address::Address;
use crate::consensus::tree_node::TreeNode;
use crate::database::state_db::StateDB;
use crate::database::IDB;
use crate::serialization::state::Account as ProtoAccount;
use crate::traits::{Encode, Exception, Proto};
use crate::util::hash::hash;
use futures::Future;
use starling::traits::Database;
use std::cmp::min;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::error::Error;
use std::iter::FromIterator;
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug)]
pub enum NodeType {
    Leaf(Account),
    Branch(StateNode),
}
pub struct LegacyTrie<DBType> {
    db: StateDB<DBType, (Vec<u8>, DBState)>,
    write_queue: Arc<Mutex<Vec<(Vec<u8>, DBState)>>>,
}

impl<DBType> LegacyTrie<DBType>
where
    DBType: IDB,
{
    pub fn new(db: StateDB<DBType, (Vec<u8>, DBState)>) -> Self {
        let write_queue = Arc::new(Mutex::new(Vec::new()));
        Self { db, write_queue }
    }
    pub fn get_account(&self, _address: Address, _root_node: &DBState) -> Option<ProtoAccount> {
        None
    }
    pub fn get(
        &self,
        root: &[u8],
        modified_accounts: &Vec<Address>,
    ) -> Result<Vec<Option<(Address, ProtoAccount)>>, Box<Error>> {
        let mut accounts = Vec::with_capacity(modified_accounts.len());
        let root_node = self.db.get_node(root)?;
        let mut node_map: HashMap<Vec<u8>, StateNode> = HashMap::new();
        match root_node {
            Some(node) => {
                let account_split = self.split_keys(modified_accounts)?;
                for (index, address) in account_split {
                    let node = self.traverse_nodes(&node, address, &mut node_map, index);
                    match node {
                        Ok(account) => {
                            accounts.push(account);
                        }
                        Err(e) => {
                            return Err(e);
                        }
                    }
                }
            }
            None => {
                return Err(Box::new(Exception::new("Root Node not found")));
            }
        }
        Ok(accounts)
    }

    fn split_keys<'a>(
        &self,
        keys: &'a Vec<Address>,
    ) -> Result<Vec<(usize, &'a Address)>, Box<Error>> {
        if keys.is_empty() {
            return Err(Box::new(Exception::new("No keys provided")));
        }
        let mut splits: Vec<(usize, &Address)> = Vec::with_capacity(keys.len());
        for i in 0..keys.len() {
            if i == 0 {
                splits.push((0, &keys[i]))
            } else {
                for j in 0..19 {
                    if keys[i - 1][j] != keys[i][j] {
                        splits.push((j, &keys[i]));
                        break;
                    }
                }
            }
        }
        Ok(splits)
    }

    pub fn insert(
        &mut self,
        root: Option<&[u8]>,
        keys: Vec<Address>,
        values: &[ProtoAccount],
    ) -> Result<Vec<u8>, Box<Error>> {
        // encode accounts and insert to db
        if keys.len() != values.len() {
            return Err(Box::new(Exception::new(
                "Keys and values have different lengths",
            )));
        }
        let split_addresses = self.split_keys(&keys)?;
        let mut node_map: BTreeMap<Vec<u8>, TreeNode> = BTreeMap::new();
        let mut ref_map: HashSet<Vec<u8>> = HashSet::new();
        // set root - empty or existing state and create base future
        let mut root_node: TreeNode;
        match root {
            Some(root_hash) => {
                if let Some(db_state) = self.db.get_node(root_hash)? {
                    if let Some(state_node) = db_state.node {
                        for child in state_node.node_refs.iter() {
                            ref_map.insert(child.1.child.clone());
                        }
                        root_node = TreeNode::new(
                            NodeType::Branch(state_node),
                            Vec::new(),
                            self.write_queue.clone(),
                            0,
                        );
                    } else {
                        return Err(Box::new(Exception::new("DB State is not a state node")));
                    }
                } else {
                    return Err(Box::new(Exception::new("DB State not found")));
                }
            }
            None => {
                let state_node = StateNode::new(Vec::new());
                root_node = TreeNode::new(
                    NodeType::Branch(state_node),
                    Vec::new(),
                    self.write_queue.clone(),
                    0,
                );
            }
        }
        let mut prev_split = 0;

        // iterate inserts
        for ((split, key), account) in split_addresses.iter().zip(values.iter()) {
            let mut offset = *split;
            let mut current_node: TreeNode;
            let mut prev_offset = 0;
            if offset > 0 {
                let n = min(prev_split, offset);
                if let Some(node) = node_map.get(&key[0..n]) {
                    current_node = node.clone();
                    if current_node.is_leaf() {
                        current_node.upgrade_to_branch()?;
                        node_map.insert(key[0..n].to_vec(), current_node.clone());
                        offset = n;
                    }
                } else {
                    current_node = root_node.clone();
                }
            } else {
                current_node = root_node.clone();
            }
            // set up to traverse states
            let mut db_state: Option<DBState>;
            if let Some(next_node) = current_node.get_next_node_location(key[offset]) {
                let mut early_out = false;
                for (i, loc) in next_node.node_location.iter().enumerate() {
                    if loc != &key[offset + i] {
                        early_out = true;
                        let new_account = Account::from_proto(account);
                        let node_hash = hash(&new_account.encode().unwrap(), 32);
                        self.db
                            .insert(&node_hash, &DBState::new(Some(new_account), None, 1))?;
                        let node_ref = NodeRef {
                            node_location: key[offset + i..key.len()].to_vec(),
                            child: node_hash,
                        };
                        let mut new_node = next_node.clone();
                        new_node.node_location =
                            next_node.node_location[i..next_node.node_location.len()].to_vec();
                        let state_node = StateNode::new(vec![node_ref, new_node]);
                        let tree_node = TreeNode::new(
                            NodeType::Branch(state_node),
                            next_node.node_location[0..i].to_vec(),
                            self.write_queue.clone(),
                            offset,
                        );
                        node_map.insert(key[0..offset + i].to_vec(), tree_node);
                        prev_split = offset + i;
                        break;
                    }
                }
                if early_out {
                    continue;
                } else {
                    prev_offset = offset;
                    offset = offset + next_node.node_location.len();
                    ref_map.remove(&next_node.child);
                    db_state = self.db.get_node(&next_node.child)?;
                }
            } else {
                // Early out if branch is empty
                let new_account = Account::from_proto(account);
                let tree_node = TreeNode::new(
                    NodeType::Leaf(new_account),
                    key[offset..key.len()].to_vec(),
                    self.write_queue.clone(),
                    offset,
                );
                node_map.insert(key[0..offset + 1].to_vec(), tree_node);
                prev_split = offset + 1;
                continue;
            }
            while let Some(state) = &db_state {
                if let Some(_prev_account) = &state.account {
                    let new_account = Account::from_proto(account);

                    let tree_node = TreeNode::new(
                        NodeType::Leaf(new_account),
                        key[prev_offset..key.len()].to_vec(),
                        self.write_queue.clone(),
                        prev_offset,
                    );
                    node_map.insert(key[0..prev_offset + 1].to_vec(), tree_node);
                    break;
                } else if let Some(node) = &state.node {
                    for child in node.node_refs.iter() {
                        ref_map.insert(child.1.child.clone());
                    }
                    let tree_node = TreeNode::new(
                        NodeType::Branch(node.clone()),
                        key[prev_offset..offset].to_vec(),
                        self.write_queue.clone(),
                        offset - 1,
                    );
                    node_map.insert(key[0..offset].to_vec(), tree_node);
                    if let Some(node_ref) = node.node_refs.get(&key[offset]) {
                        // check key compression
                        let mut early_out = false;
                        for (i, loc) in node_ref.node_location.iter().enumerate() {
                            if loc != &key[offset + i] {
                                early_out = true;
                                let new_account = Account::from_proto(account);
                                let node_hash = hash(&new_account.encode().unwrap(), 32);
                                self.db.insert(
                                    &node_hash,
                                    &DBState::new(Some(new_account), None, 1),
                                )?;
                                let new_node_ref = NodeRef {
                                    node_location: key[offset + i..key.len()].to_vec(),
                                    child: node_hash,
                                };
                                let mut new_node = node_ref.clone();
                                new_node.node_location = node_ref.node_location
                                    [i..node_ref.node_location.len()]
                                    .to_vec();
                                let state_node = StateNode::new(vec![new_node, new_node_ref]);
                                let tree_node = TreeNode::new(
                                    NodeType::Branch(state_node),
                                    node_ref.node_location[0..i].to_vec(),
                                    self.write_queue.clone(),
                                    offset,
                                );
                                node_map.insert(key[0..offset + i].to_vec(), tree_node);
                                break;
                            }
                        }
                        if early_out {
                            break;
                        }
                        if let Some(next_node) = self.db.get_node(&node_ref.child)? {
                            ref_map.remove(&node_ref.child);
                            if next_node.account.is_none() {
                                prev_offset = offset;
                                offset += node_ref.node_location.len();
                                db_state = Some(next_node);
                                continue;
                            } else {
                                let new_account = Account::from_proto(account);
                                let tree_node = TreeNode::new(
                                    NodeType::Leaf(new_account),
                                    key[offset..key.len()].to_vec(),
                                    self.write_queue.clone(),
                                    offset,
                                );
                                node_map.insert(key[0..offset+1].to_vec(), tree_node);
                                prev_split = offset;
                                db_state = None;
                                continue;
                            }
                        } else {
                            let new_account = Account::from_proto(account);
                            let tree_node = TreeNode::new(
                                NodeType::Leaf(new_account),
                                key[offset..key.len()].to_vec(),
                                self.write_queue.clone(),
                                prev_offset,
                            );
                            node_map.insert(key[0..prev_offset + 1].to_vec(), tree_node);
                            prev_split = offset;
                            db_state = None;
                            continue;
                        }
                    } else {
                        let new_account = Account::from_proto(account);
                        let tree_node = TreeNode::new(
                            NodeType::Leaf(new_account),
                            key[offset..key.len()].to_vec(),
                            self.write_queue.clone(),
                            prev_offset,
                        );
                        node_map.insert(key[0..prev_offset + 1].to_vec(), tree_node);
                        prev_split = offset;
                        db_state = None;
                        continue;
                    }
                } else {
                    return Err(Box::new(Exception::new(
                        "Unable to find node, corrupted tree",
                    )));
                    // we got nothing
                }
            }
        }
        let cln = node_map.clone();
        let mut futures = Vec::from_iter(cln.iter());
        let mut curr_node: Option<(&Vec<u8>, &TreeNode)> = futures.pop();
        while let Some(node) = &curr_node {
            if let Some(removed_node) = node_map.remove(node.0) {
                if removed_node.parent == 0 {
                    root_node.add_future(&removed_node);
                } else if let Some(tree_node) = node_map.get_mut(&node.0[0..node.1.parent]) {
                    tree_node.add_future(&removed_node);
                }
                curr_node = futures.pop();
            } else {
                return Err(Box::new(Exception::new(
                    "Error constructing tree, cannot resolve futures",
                )));
            }
        }
        let tree_root = root_node.wait();
        match tree_root {
            Ok(root) => {
                for (key, value) in self.write_queue.lock().unwrap().iter() {
                    self.db.insert(&key, &value)?;
                }
                self.write_queue.lock().unwrap().clear();
                self.update_refs(ref_map)?;
                self.db.batch_write()?;
                Ok(root.child)
            }
            Err(_e) => Err(Box::new(Exception::new("Error generating new root"))),
        }
    }

    pub fn remove(&mut self, root: &[u8]) -> Result<(), Box<Error>> {
        let mut pending_keys: Vec<Vec<u8>> = Vec::new();
        let mut db_state = self.db.get_node(root)?;
        let mut key = root.to_vec();
        while let Some(state) = &db_state {
            if state.ref_count == 1 {
                self.db.remove(&key)?;
                match &state.node {
                    Some(node) => {
                        for node_ref in &node.node_refs {
                            pending_keys.push(node_ref.1.child.clone());
                        }
                    }
                    None => {}
                }
            } else {
                let mut new_state = state.clone();
                new_state.ref_count -= 1;
                self.db.insert(&key, &new_state)?;
            }
            if let Some(next_key) = pending_keys.pop() {
                key = next_key;
                db_state = self.db.get_node(&key)?;
            } else {
                break;
            }
        }
        self.db.batch_write()?;
        Ok(())
    }

    fn traverse_nodes(
        &self,
        root: &DBState,
        address: &Address,
        map: &mut HashMap<Vec<u8>, StateNode>,
        split: usize,
    ) -> Result<Option<(Address, ProtoAccount)>, Box<Error>> {
        let mut state: Option<DBState> = Some(root.clone());;
        let mut offset = split;
        if offset > 0 {
            if let Some(node) = map.get(&address[0..offset]) {
                if let Some(node_ref) = node.node_refs.get(&address[offset]) {
                    if let Some(next_node) = self.db.get_node(&node_ref.child)? {
                        offset += node_ref.node_location.len();
                        state = Some(next_node);
                    } else {
                        return Err(Box::new(Exception::new(
                            " First Unable to find node, corrupted tree",
                        )));
                    }
                }
            }
        }
        while let Some(db_state) = &state {
            if let Some(account) = &db_state.account {
                return Ok(Some((*address, account.to_proto()?)));
            //we have an account
            } else if let Some(node) = &db_state.node {
                map.insert(address[0..offset].to_vec(), node.clone());
                if let Some(node_ref) = node.node_refs.get(&address[offset]) {
                    if let Some(next_node) = self.db.get_node(&node_ref.child)? {
                        offset += node_ref.node_location.len();
                        state = Some(next_node);
                        continue;
                    } else {
                        return Err(Box::new(Exception::new(
                            "Second Unable to find node, corrupted tree",
                        )));
                    }
                } else {
                    return Err(Box::new(Exception::new(
                        "Third Unable to find node, corrupted tree",
                    )));
                }
            } else {
                return Err(Box::new(Exception::new(
                    "Fourth Unable to find node, corrupted tree",
                )));
                // we got nothing
            }
        }
        Ok(None)
    }
    //Naive Implementation to test key identification logic
    fn update_refs(&mut self, nodes: HashSet<Vec<u8>>) -> Result<(), Box<Error>> {
        let refs: Vec<Vec<u8>> = nodes.iter().cloned().collect();
        for node in refs {
            if let Some(mut db_state) = self.db.get_node(&node)? {
                db_state.ref_count = db_state.ref_count + 1;
                self.db.insert(&node, &db_state)?;
            } else {
                return Err(Box::new(Exception::new(
                    "Could not increment reference count",
                )));
            }
        }
        Ok(())
    }
}
#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::account::account::Account;
    use crate::account::node_ref::NodeRef;
    use crate::common::address::ValidAddress;
    use crate::common::exodus_block::ExodusBlock;
    use crate::common::transaction::Transaction;
    use crate::database::mock::RocksDBMock;
    use crate::traits::{Decode, Encode};
    use crate::util::hash::hash;
    use std::env;
    use std::fs::File;
    use std::io::Read;
    use std::path::PathBuf;
    use rand::{Rng,thread_rng};

    #[test]
    fn it_gets_items_from_a_tree_of_depth_1() {
        let path = PathBuf::new();
        let mut state_db: StateDB<RocksDBMock> = StateDB::new(path, None).unwrap();
        let mut accounts: Vec<NodeRef> = Vec::with_capacity(256);
        for i in 0..255 {
            let db_state = DBState::new(
                Some(Account {
                    balance: i * 100,
                    nonce: i as u32,
                }),
                None,
                1,
            );
            let hash = hash(db_state.encode().unwrap().as_ref(), 32);
            let _ = state_db.insert(&hash, &db_state);
            let location = vec![
                i as u8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            ];
            let node_ref = NodeRef::new(&location, &hash);
            accounts.push(node_ref);
        }
        let state_node = StateNode::new(accounts);
        let state_hash = hash(state_node.encode().unwrap().as_ref(), 32);
        let db_state = DBState::new(None, Some(state_node), 1);
        let _ = state_db.insert(&state_hash, &db_state);
        let _ = state_db.batch_write();
        let legacy_trie = LegacyTrie::new(state_db);
        let returned_accounts = legacy_trie.get(
            &state_hash,
            &vec![
                [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
                [12, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            ],
        );
        match returned_accounts {
            Ok(vec) => {
                assert_eq!(vec.len(), 2);
                if let Some(account) = &vec[0] {
                    assert_eq!(account.1.balance, 0);
                    assert_eq!(account.1.nonce, 0);
                } else {
                    println!("Node not found");
                    unimplemented!()
                }
                if let Some(account) = &vec[1] {
                    assert_eq!(account.1.balance, 1200);
                    assert_eq!(account.1.nonce, 12);
                } else {
                    println!("Node not found");
                    unimplemented!()
                }
            }
            Err(e) => {
                println!("Error: {:?}", e);
                unimplemented!()
            }
        }
    }

    #[test]
    fn it_gets_an_item_from_a_depth_greater_than_one() {
        let path = PathBuf::new();
        let mut state_db: StateDB<RocksDBMock> = StateDB::new(path, None).unwrap();
        let first_account = DBState::new(
            Some(Account {
                balance: 100,
                nonce: 1,
            }),
            None,
            1,
        );
        let first_hash = hash(first_account.encode().unwrap().as_ref(), 32);
        let _ = state_db.insert(&first_hash, &first_account);
        let first_location = vec![0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        let first_node_ref = NodeRef::new(&first_location, &first_hash);
        let second_account = DBState::new(
            Some(Account {
                balance: 200,
                nonce: 2,
            }),
            None,
            1,
        );
        let second_hash = hash(second_account.encode().unwrap().as_ref(), 32);
        let _ = state_db.insert(&second_hash, &second_account);
        let second_location = vec![1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        let second_node_ref = NodeRef::new(&second_location, &second_hash);
        let third_account = DBState::new(
            Some(Account {
                balance: 300,
                nonce: 3,
            }),
            None,
            1,
        );
        let third_hash = hash(third_account.encode().unwrap().as_ref(), 32);
        let _ = state_db.insert(&third_hash, &third_account);
        let third_location = vec![1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        let third_node_ref = NodeRef::new(&third_location, &third_hash);
        let second_level_refs = vec![first_node_ref, second_node_ref];
        let second_state_node = StateNode::new(second_level_refs);
        let second_state_node_state = DBState::new(None, Some(second_state_node), 1);
        let second_state_node_hash = hash(&second_state_node_state.encode().unwrap(), 32);
        let _ = state_db.insert(&second_state_node_hash, &second_state_node_state);
        let first_level_node = NodeRef::new(&vec![0], &second_state_node_hash);
        let root_node_refs = vec![first_level_node, third_node_ref];
        let root_state_node = StateNode::new(root_node_refs);
        let root_db_state = DBState::new(None, Some(root_state_node), 1);
        let root_hash = hash(&root_db_state.encode().unwrap(), 32);
        let _ = state_db.insert(&root_hash, &root_db_state);
        let _ = state_db.batch_write();
        let tree = LegacyTrie::new(state_db);
        let addresses = vec![
            [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            [0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            [1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        ];
        let returned_accounts = tree.get(&root_hash, &addresses);
        match returned_accounts {
            Ok(vec) => {
                assert_eq!(vec.len(), 3);
                // check integrity of returned accounts
                for i in 0..vec.len() {
                    match &vec[i] {
                        Some(account) => {
                            assert_eq!(account.1.balance as usize, (i + 1) * 100);
                            assert_eq!(account.1.nonce as usize, i + 1);
                        }
                        None => unimplemented!(),
                    }
                }
            }
            Err(e) => {
                println!("Error: {:?}", e);
                unimplemented!()
            }
        }
    }
    #[test]
    fn it_gets_from_a_tree_with_compressed_branches() {
        let path = PathBuf::new();
        let mut state_db: StateDB<RocksDBMock> = StateDB::new(path, None).unwrap();
        let first_account = DBState::new(
            Some(Account {
                balance: 100,
                nonce: 1,
            }),
            None,
            1,
        );
        let first_hash = hash(first_account.encode().unwrap().as_ref(), 32);
        let _ = state_db.insert(&first_hash, &first_account);
        let first_location = vec![0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        let first_node_ref = NodeRef::new(&first_location, &first_hash);
        let second_account = DBState::new(
            Some(Account {
                balance: 200,
                nonce: 2,
            }),
            None,
            1,
        );
        let second_hash = hash(second_account.encode().unwrap().as_ref(), 32);
        let _ = state_db.insert(&second_hash, &second_account);
        let second_location = vec![1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        let second_node_ref = NodeRef::new(&second_location, &second_hash);
        let third_account = DBState::new(
            Some(Account {
                balance: 300,
                nonce: 3,
            }),
            None,
            1,
        );
        let third_hash = hash(third_account.encode().unwrap().as_ref(), 32);
        let _ = state_db.insert(&third_hash, &third_account);
        let third_location = vec![1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        let third_node_ref = NodeRef::new(&third_location, &third_hash);
        let second_level_refs = vec![first_node_ref, second_node_ref];
        let second_state_node = StateNode::new(second_level_refs);
        let second_state_node_state = DBState::new(None, Some(second_state_node), 1);
        let second_state_node_hash = hash(&second_state_node_state.encode().unwrap(), 32);
        let _ = state_db.insert(&second_state_node_hash, &second_state_node_state);
        let first_level_node = NodeRef::new(&vec![0, 0, 0, 0, 0], &second_state_node_hash);
        let root_node_refs = vec![first_level_node, third_node_ref];
        let root_state_node = StateNode::new(root_node_refs);
        let root_db_state = DBState::new(None, Some(root_state_node), 1);
        let root_hash = hash(&root_db_state.encode().unwrap(), 32);
        let _ = state_db.insert(&root_hash, &root_db_state);
        let _ = state_db.batch_write();
        let tree = LegacyTrie::new(state_db);
        let addresses = vec![
            [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            [0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            [1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        ];
        let returned_accounts = tree.get(&root_hash, &addresses);
        match returned_accounts {
            Ok(vec) => {
                assert_eq!(vec.len(), 3);
                println!("accounts: {:?}", vec);
                // check integrity of returned accounts
                for i in 0..vec.len() {
                    match &vec[i] {
                        Some(account) => {
                            assert_eq!(account.1.balance as usize, (i + 1) * 100);
                            assert_eq!(account.1.nonce as usize, i + 1);
                        }
                        None => unimplemented!(),
                    }
                }
            }
            Err(e) => {
                println!("Error: {:?}", e);
                unimplemented!()
            }
        }
    }
    #[test]
    fn it_calculates_the_split_points_for_keys() {
        let path = PathBuf::new();
        let state_db: StateDB<RocksDBMock> = StateDB::new(path, None).unwrap();
        let trie = LegacyTrie::new(state_db);
        let address_bytes = vec![
            [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            [0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            [1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        ];
        let mut addresses: Vec<Address> = Vec::new();
        for address in address_bytes {
            addresses.push(Address::from_bytes(&address));
        }
        let split_addresses = trie.split_keys(&addresses).unwrap();
        assert_eq!(split_addresses.len(), 3);
        assert_eq!(split_addresses[0].0, 0);
        assert_eq!(split_addresses[1].0, 1);
        assert_eq!(split_addresses[2].0, 0);
    }
    #[test]
    fn it_inserts_256_keys_with_different_first_bytes_into_empty_tree_and_retrieves_them() {
        let path = PathBuf::new();
        let state_db: StateDB<RocksDBMock> = StateDB::new(path, None).unwrap();
        let mut tree = LegacyTrie::new(state_db);
        let mut accounts = Vec::with_capacity(256);
        let mut addresses = Vec::with_capacity(256);
        for i in 0..256 {
            let account = Account {
                balance: i * 100,
                nonce: i as u32,
            };
            let address = Address::from([
                i as u8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            ]);
            accounts.push(account.to_proto().unwrap());
            addresses.push(address);
        }
        let root_hash = tree.insert(None, addresses.clone(), &accounts).unwrap();
        let retrieved_accounts = tree.get(&root_hash, &addresses).unwrap();
        assert_eq!(retrieved_accounts.len(), 256);
        for (i, (opt, original_address)) in
            retrieved_accounts.iter().zip(addresses.iter()).enumerate()
        {
            assert!(opt.is_some());
            match opt {
                Some((add, account)) => {
                    assert_eq!(add, original_address);
                    assert_eq!(account.balance, (i * 100) as u64);
                    assert_eq!(account.nonce, i as u32);
                }
                None => {}
            }
        }
    }
    #[test]
    fn it_inserts_and_retrieves_a_key_from_an_existing_tree() {
        let path = PathBuf::new();
        let mut state_db: StateDB<RocksDBMock> = StateDB::new(path, None).unwrap();
        let first_account = DBState::new(
            Some(Account {
                balance: 100,
                nonce: 1,
            }),
            None,
            1,
        );
        let first_hash = hash(first_account.encode().unwrap().as_ref(), 32);
        let _ = state_db.insert(&first_hash, &first_account);
        let first_location = vec![0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        let first_node_ref = NodeRef::new(&first_location, &first_hash);
        let second_account = DBState::new(
            Some(Account {
                balance: 200,
                nonce: 2,
            }),
            None,
            1,
        );
        let second_hash = hash(second_account.encode().unwrap().as_ref(), 32);
        let _ = state_db.insert(&second_hash, &second_account);
        let second_location = vec![1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        let second_node_ref = NodeRef::new(&second_location, &second_hash);
        let third_account = DBState::new(
            Some(Account {
                balance: 300,
                nonce: 3,
            }),
            None,
            1,
        );
        let third_hash = hash(third_account.encode().unwrap().as_ref(), 32);
        let _ = state_db.insert(&third_hash, &third_account);
        let third_location = vec![1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        let third_node_ref = NodeRef::new(&third_location, &third_hash);
        let second_level_refs = vec![first_node_ref, second_node_ref];
        let second_state_node = StateNode::new(second_level_refs);
        let second_state_node_state = DBState::new(None, Some(second_state_node), 1);
        let second_state_node_hash = hash(&second_state_node_state.encode().unwrap(), 32);
        let _ = state_db.insert(&second_state_node_hash, &second_state_node_state);
        let first_level_node = NodeRef::new(&vec![0], &second_state_node_hash);
        let root_node_refs = vec![first_level_node, third_node_ref];
        let root_state_node = StateNode::new(root_node_refs);
        let root_db_state = DBState::new(None, Some(root_state_node), 1);
        let root_hash = hash(&root_db_state.encode().unwrap(), 32);
        let _ = state_db.insert(&root_hash, &root_db_state);
        let _ = state_db.batch_write();
        let mut tree = LegacyTrie::new(state_db);
        let account = Account {
            balance: 500,
            nonce: 2,
        };
        let account_proto = account.to_proto().unwrap();
        let address_bytes = vec![
            [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            [0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            [1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        ];
        let mut addresses: Vec<Address> = Vec::new();
        for address in address_bytes {
            addresses.push(Address::from_bytes(&address));
        }
        let accounts = vec![
            account_proto.clone(),
            account_proto.clone(),
            account_proto.clone(),
        ];
        let result = tree.insert(Some(&root_hash), addresses.clone(), &accounts);
        let new_root = result.unwrap();
        assert_ne!(&new_root, &root_hash);
        let returned_accounts = tree.get(&new_root, &addresses);
        match returned_accounts {
            Ok(vec) => {
                assert_eq!(vec.len(), 3);
                // check integrity of returned accounts
                for i in 0..vec.len() {
                    match &vec[i] {
                        Some(account) => {
                            assert_eq!(account.1.balance as usize, 500);
                            assert_eq!(account.1.nonce as usize, 2);
                        }
                        None => unimplemented!(),
                    }
                }
            }
            Err(e) => {
                println!("Error: {:?}", e);
                unimplemented!()
            }
        }
    }

    #[test]
    fn it_inserts_a_node_into_a_compressed_branch() {
        let path = PathBuf::new();
        let state_db: StateDB<RocksDBMock> = StateDB::new(path, None).unwrap();
        let mut tree = LegacyTrie::new(state_db);
        let address_bytes = vec![
            [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            [0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            [0, 1, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            [0, 1, 2, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            [1, 1, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            [1, 1, 0, 0, 4, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            [1, 2, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        ];
        let mut addresses: Vec<Address> = Vec::new();
        for address in address_bytes {
            addresses.push(Address::from_bytes(&address));
        }
        addresses.sort();
        let mut account_vec = Vec::with_capacity(4);
        for i in 1..8 {
            let account = Account {
                balance: i * 100,
                nonce: i as u32,
            };
            account_vec.push(account.to_proto().unwrap());
        }

        let result = tree.insert(None, addresses.clone(), &account_vec);
        let new_root = result.unwrap();
        let accounts = tree.get(&new_root, &addresses).unwrap();
        assert_eq!(accounts.len(), 7);
        for (i, (opt, original_address)) in accounts.iter().zip(addresses.iter()).enumerate() {
            assert!(opt.is_some());
            match opt {
                Some((add, account)) => {
                    assert_eq!(add, original_address);
                    assert_eq!(account.balance, ((i + 1) * 100) as u64);
                    assert_eq!(account.nonce, (i + 1) as u32);
                }
                None => {}
            }
        }
        let new_address =
            Address::from_bytes(&[0, 1, 0, 0, 0, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        let new_account = Account::new(1100, 1);
        let vec = vec![new_account.to_proto().unwrap()];
        let new_result = tree
            .insert(Some(&new_root), vec![new_address], &vec)
            .unwrap();
        addresses.push(new_address);
        addresses.sort();
        let accounts = tree.get(&new_result, &addresses).unwrap();
        assert_eq!(accounts.len(), 8);
        for (i, (opt, original_address)) in accounts.iter().zip(addresses.iter()).enumerate() {
            assert!(opt.is_some());
            match opt {
                Some((add, account)) => {
                    assert_eq!(add, original_address);
                    if i == 3 {
                        assert_eq!(account.balance, 1100);
                        assert_eq!(account.nonce, 1);
                    }
                }
                None => {}
            }
        }
    }

    #[test]
    fn it_matches_typescript_world_state_for_exodus_block() {
        //let path_to_exodus = PathBuf::from("./../../data/exodusBlock.dat");
        let mut path = env::current_dir().unwrap();
        path.push("data/exodusBlock.dat");
        let mut exodus_file = File::open(path).unwrap();
        let mut exodus_buf = Vec::new();
        exodus_file.read_to_end(&mut exodus_buf).unwrap();
        let exodus = ExodusBlock::decode(&exodus_buf).unwrap();
        let mut keypairs: Vec<(Address, ProtoAccount)> = Vec::with_capacity(12000);
        let mut addresses: Vec<Address> = Vec::with_capacity(12000);
        let mut accounts: Vec<ProtoAccount> = Vec::with_capacity(12000);
        match &exodus.txs {
            Some(tx_vec) => {
                for tx in tx_vec {
                    let amount: u64 = tx.get_amount();
                    let nonce: u32;
                    if let Some(tx_nonce) = tx.get_nonce() {
                        nonce = tx_nonce;
                    } else {
                        break;
                    }
                    if let Some(add) = tx.get_to() {
                        keypairs.push((add, Account::new(amount, nonce).to_proto().unwrap()));
                    } else {
                        break;
                    }
                }
            }
            None => {}
        }
        keypairs.sort_by(|a, b| a.0.cmp(&b.0));
        for (key, value) in keypairs.clone() {
            addresses.push(key);
            accounts.push(value);
        }
        let db_path = PathBuf::new();
        let state_db: StateDB<RocksDBMock> = StateDB::new(db_path, None).unwrap();
        let mut tree = LegacyTrie::new(state_db);
        let root = tree.insert(None, addresses.clone(), &accounts).unwrap();
        let expected_root = vec![
            57, 92, 11, 69, 43, 154, 183, 169, 122, 56, 191, 8, 12, 60, 185, 124, 155, 185, 54, 47,
            143, 83, 11, 147, 238, 198, 92, 130, 35, 27, 188, 134,
        ];
        assert_eq!(root, expected_root);
        let retrieved = tree.get(&root, &addresses).unwrap();
        for (ret, keypair) in retrieved.iter().zip(keypairs.iter()) {
            assert!(ret.is_some());
            match ret {
                Some((add, account)) => {
                    assert_eq!(add, &keypair.0);
                    assert_eq!(account.balance, keypair.1.balance);
                    assert_eq!(account.nonce, keypair.1.nonce);
                }
                None => {}
            }
        }
    }
    #[test]
    fn it_increments_the_reference_count_for_untraversed_branches_and_prunes() {
        let db_path = PathBuf::new();
        let state_db: StateDB<RocksDBMock> = StateDB::new(db_path, None).unwrap();
        let mut tree = LegacyTrie::new(state_db);
        let address_bytes = vec![
            [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            [0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            [0, 1, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            [0, 1, 2, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            [1, 1, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            [1, 1, 0, 0, 4, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            [1, 2, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        ];
        let mut addresses: Vec<Address> = Vec::new();
        for address in address_bytes {
            addresses.push(Address::from_bytes(&address));
        }
        addresses.sort();
        let mut account_vec = Vec::with_capacity(4);
        for i in 1..8 {
            let account = Account {
                balance: i * 100,
                nonce: i as u32,
            };
            account_vec.push(account.to_proto().unwrap());
        }

        let result = tree.insert(None, addresses.clone(), &account_vec);
        let new_root = result.unwrap();
        let new_address =
            Address::from_bytes(&[0, 1, 0, 0, 0, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        let new_account = Account::new(1100, 1);
        let vec = vec![new_account.to_proto().unwrap()];
        let new_result = tree
            .insert(Some(&new_root), vec![new_address], &vec)
            .unwrap();
        addresses.push(new_address);
        addresses.sort();
        let _ = tree.remove(&new_root);
        assert!(tree.db.get_node(&new_root).is_err());
        let accounts = tree.get(&new_result, &addresses).unwrap();
        assert_eq!(accounts.len(), 8);
        for (i, (opt, original_address)) in accounts.iter().zip(addresses.iter()).enumerate() {
            assert!(opt.is_some());
            match opt {
                Some((add, account)) => {
                    assert_eq!(add, original_address);
                    if i == 3 {
                        assert_eq!(account.balance, 1100);
                        assert_eq!(account.nonce, 1);
                    }
                }
                None => {}
            }
        }
    }

    #[test]
    fn it_can_prune_a_real_tree_after_numerous_inserts() {
        let db_path = PathBuf::new();
        let state_db: StateDB<RocksDBMock> = StateDB::new(db_path, None).unwrap();
        let mut tree = LegacyTrie::new(state_db);
        let (root,addresses) = initiate_exodus_state(&mut tree);
        let mut rng = thread_rng();
        let mut current_root = root;
        let mut roots = vec![current_root.clone()];
        for _ in 0..100 {
            let num = rng.gen_range(1,100);
            let mut changed_addresses = select_random_accounts(&mut rng, &addresses, num);
            changed_addresses.sort();
            let changed_accounts = tree.get(&current_root,&changed_addresses).unwrap();
            let mut updated_accounts = Vec::with_capacity(changed_accounts.len());
            for modified in changed_accounts {
                if let Some(modified_account) = modified {
                let mut new_account = modified_account.1;
                new_account.balance += 100;
                new_account.nonce +=1;
                updated_accounts.push(new_account);
                } 
            }
            let new_root = tree.insert(Some(&current_root), changed_addresses.clone(), &updated_accounts).unwrap();
            roots.push(new_root.clone());
            current_root = new_root;
        }
        let retrieved = tree.get(&current_root,&addresses);
        assert!(retrieved.is_ok());
        let last_root = roots.pop();
        assert_eq!(Some(current_root),last_root);
        assert_eq!(roots.len(),100);
    }

    #[test]
    fn it_can_update_accounts_correctly_in_real_tree() {
        let address_bytes = vec![[0, 184, 45, 82, 76, 76, 245, 63, 56, 195, 39, 82, 177, 210, 89, 69, 92, 228, 154, 180], [4, 202, 92, 91, 92, 23, 108, 236, 249, 159, 71, 113, 1, 152, 195, 240, 110, 23, 160, 110], [9, 118, 136, 113, 189, 135, 84, 215, 3, 184, 12, 46, 239, 216, 52, 53, 222, 227, 195, 220], [25, 8, 48, 4, 188, 150, 120, 105, 88, 199, 220, 50, 253, 143, 241, 100, 229, 188, 8, 112], [44, 145, 42, 145, 209, 34, 118, 224, 204, 77, 85, 90, 249, 11, 239, 74, 44, 200, 144, 9], [56, 196, 213, 249, 206, 148, 250, 50, 161, 52, 165, 157, 98, 191, 230, 56, 52, 107, 18, 139], [62, 185, 33, 237, 70, 140, 178, 215, 224, 176, 42, 225, 227, 0, 98, 119, 74, 9, 120, 252], [63, 86, 219, 226, 161, 32, 122, 181, 254, 227, 42, 47, 135, 17, 0, 253, 164, 1, 99, 0], [63, 183, 251, 131, 137, 192, 126, 94, 45, 127, 39, 169, 31, 162, 233, 122, 239, 52, 103, 115], [67, 70, 229, 239, 103, 112, 35, 243, 228, 118, 71, 63, 151, 213, 147, 50, 55, 89, 209, 63], [108, 158, 174, 104, 242, 89, 239, 32, 143, 191, 194, 138, 252, 100, 19, 213, 223, 87, 53, 48], [129, 165, 238, 87, 79, 108, 189, 29, 42, 81, 80, 232, 120, 171, 25, 118, 64, 126, 213, 146], [131, 255, 82, 147, 112, 87, 197, 56, 225, 27, 30, 108, 233, 121, 109, 214, 190, 92, 56, 8], [132, 103, 151, 195, 214, 130, 244, 164, 37, 2, 69, 140, 5, 147, 11, 43, 98, 132, 163, 44], [136, 197, 141, 75, 34, 168, 185, 128, 21, 147, 51, 42, 91, 216, 77, 68, 216, 80, 20, 236], [139, 130, 218, 106, 46, 221, 103, 113, 145, 61, 143, 69, 112, 16, 213, 217, 92, 229, 240, 61], [151, 152, 0, 10, 65, 100, 118, 241, 81, 181, 216, 17, 106, 138, 87, 14, 221, 88, 249, 34], [158, 74, 97, 253, 98, 14, 44, 65, 80, 115, 183, 31, 95, 204, 163, 71, 247, 173, 44, 33], [169, 79, 124, 116, 142, 150, 242, 176, 150, 207, 205, 166, 243, 181, 188, 97, 5, 51, 177, 63], [196, 45, 34, 70, 209, 31, 153, 190, 236, 126, 230, 238, 218, 34, 9, 2, 16, 60, 19, 171], [201, 73, 63, 7, 121, 164, 32, 70, 54, 107, 114, 67, 180, 5, 200, 249, 30, 46, 28, 43], [204, 103, 211, 92, 186, 80, 52, 51, 242, 20, 172, 205, 154, 33, 226, 226, 50, 83, 244, 181], [207, 226, 50, 111, 243, 225, 57, 190, 79, 38, 62, 168, 89, 238, 56, 204, 25, 124, 194, 19], [251, 89, 230, 192, 154, 85, 91, 111, 230, 209, 27, 205, 83, 35, 238, 48, 233, 4, 136, 168], [255, 237, 6, 60, 87, 23, 97, 249, 138, 248, 127, 149, 40, 191, 2, 123, 54, 129, 2, 198]];
        let mut addresses: Vec<Address> = Vec::new();
        for address in address_bytes {
            addresses.push(Address::from_bytes(&address));
        }
        addresses.sort();
        let mut account_vec = Vec::with_capacity(4);
        for i in 1..addresses.len()+1 {
            let account = Account {
                balance: i as u64 * 100,
                nonce: i as u32,
            };
            account_vec.push(account.to_proto().unwrap());
        }
        let db_path = PathBuf::new();
        let state_db: StateDB<RocksDBMock> = StateDB::new(db_path, None).unwrap();
        let mut tree = LegacyTrie::new(state_db);
        let (root,_) = initiate_exodus_state(&mut tree);
        let new_root = tree.insert(Some(&root),addresses,&account_vec);
        println!("Root: {:?}",new_root);
    }


    // Helper Functions for easier construction of tests
    fn select_random_accounts(rng: &mut rand::prelude::ThreadRng, accounts: &Vec<Address>, number: usize) ->  Vec<Address> {
        assert!(number<=accounts.len());
        let mut add_set: BTreeMap<Address,()> = BTreeMap::new();
        let mut address_vec = Vec::with_capacity(number);
        for _ in 0..number {
            let index = rng.gen_range(0, accounts.len());
            add_set.insert(accounts[index].clone(),());
        }
        for add in add_set {
            address_vec.push(add.0);
        }
        address_vec
    }

    fn initiate_exodus_state(tree: &mut LegacyTrie<RocksDBMock>) -> (Vec<u8>,Vec<Address>) {
        let mut path = env::current_dir().unwrap();
        path.push("data/exodusBlock.dat");
        let mut exodus_file = File::open(path).unwrap();
        let mut exodus_buf = Vec::new();
        exodus_file.read_to_end(&mut exodus_buf).unwrap();
        let exodus = ExodusBlock::decode(&exodus_buf).unwrap();
        let mut keypairs: Vec<(Address, ProtoAccount)> = Vec::with_capacity(12000);
        let mut addresses: Vec<Address> = Vec::with_capacity(12000);
        let mut accounts: Vec<ProtoAccount> = Vec::with_capacity(12000);
        match &exodus.txs {
            Some(tx_vec) => {
                for tx in tx_vec {
                    let amount: u64 = tx.get_amount();
                    let nonce: u32;
                    if let Some(tx_nonce) = tx.get_nonce() {
                        nonce = tx_nonce;
                    } else {
                        break;
                    }
                    if let Some(add) = tx.get_to() {
                        keypairs.push((add, Account::new(amount, nonce).to_proto().unwrap()));
                    } else {
                        break;
                    }
                }
            }
            None => {}
        }
        keypairs.sort_by(|a, b| a.0.cmp(&b.0));
        for (key, value) in keypairs.clone() {
            addresses.push(key);
            accounts.push(value);
        }
        addresses.sort();
        (tree.insert(None, addresses.clone(), &accounts).unwrap(),addresses)
    }
}
