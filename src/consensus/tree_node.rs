use crate::account::db_state::DBState;
use crate::account::node_ref::NodeRef;
use crate::account::state_node::StateNode;
use crate::consensus::legacy_trie::NodeType;
use crate::traits::{Encode, Exception};
use crate::util::hash::hash;
use futures::future::Future;
use std::error::Error;
use std::fmt::{Debug, Formatter, Result as FormatResult};
use std::sync::{Arc, Mutex};
use tokio::prelude::*;
/// Node that represents a future that will resolve to a new node in a Merkle Patricia Trie
#[derive(Clone)]
pub struct TreeNode {
    node: NodeType,
    location: Vec<u8>,
    /// A number representing the how much of the current node's physical location represents the node's parent
    /// #### Example
    /// Suppose we have a key
    /// ```
    /// let key = [0,1,2,3,4,5,6];
    /// ```
    /// The location in the tree for this node's parent would then be:
    /// ```
    /// key[0..parent];
    /// ```
    /// With ```parent=3``` for example, the parent's location would be
    /// ```
    /// key[0..3] == [0,1,2]
    /// ```
    pub parent: usize,
    futures: Vec<TreeNode>,
    write_queue: Arc<Mutex<Vec<(Vec<u8>, DBState)>>>,
}

impl TreeNode {
    pub fn new(
        node: NodeType,
        location: Vec<u8>,
        write_queue: Arc<Mutex<Vec<(Vec<u8>, DBState)>>>,
        parent: usize,
    ) -> Self {
        Self {
            node,
            location,
            parent,
            futures: Vec::new(),
            write_queue,
        }
    }
    /// Adds a sub-future to this node, which will be resolved before this one
    pub fn add_future(&mut self, tree_node: &TreeNode) {
        self.futures.push(tree_node.clone());
    }

    /// Retrieves a reference to the [NodeRef](crate::account::node_ref::NodeRef) at the next location specified by the key
    pub fn get_next_node_location(&self, key: u8) -> Option<&NodeRef> {
        match &self.node {
            NodeType::Leaf(_) => None,
            NodeType::Branch(node) => {
                if let Some(node_ref) = node.node_refs.get(&key) {
                    Some(&node_ref)
                } else {
                    None
                }
            }
        }
    }
    /// Returns a reference to the [Node](crate::consensus::legacy_trie::NodeType) held by this struct
    pub fn get_node(&self) -> &NodeType {
        &self.node
    }
    /// Returns the physical location in the tree of this Node
    pub fn get_location(&self) -> &Vec<u8> {
        &self.location
    }
    /// Check if contained Node is a leaf node
    pub fn is_leaf(&self) -> bool {
        match self.node {
            NodeType::Leaf(_) => true,
            NodeType::Branch(_) => false,
        }
    }
    /// Upgrades a NodeType::Leaf to NodeType::Branch
    pub fn upgrade_to_branch(&mut self) -> Result<(), Box<Error>> {
        match self.node {
            NodeType::Leaf(account) => {
                let value = account.encode()?;
                let hash = hash(&value, 32);
                let node_ref = NodeRef::new(&self.location[1..self.location.len()].to_vec(), &hash);
                let state_node = StateNode::new(vec![node_ref]);
                let db_state = DBState::new(Some(account.clone()), None, 1);
                let guard = self.write_queue.lock();
                match guard {
                    Ok(mut vec) => {
                        vec.push((hash, db_state));
                    }
                    Err(_e) => {
                        return Err(Box::new(Exception::new("Poison error")));
                    }
                }
                self.node = NodeType::Branch(state_node);
                self.location = self.location[0..1].to_vec();
            }
            NodeType::Branch(_) => {
                return Err(Box::new(Exception::new("Node is already a branch")));
            }
        }
        Ok(())
    }
}

impl Future for TreeNode {
    type Item = NodeRef;
    type Error = Box<Error>;
    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let curr_node = self.node.clone();
        match curr_node {
            NodeType::Leaf(node) => {
                let node_hash = hash(&node.encode()?, 32);
                let db_state = DBState::new(Some(node.clone()), None, 1);
                let node_ref = NodeRef::new(&self.location, &node_hash);
                let guard = self.write_queue.lock();
                match guard {
                    Ok(mut vec) => {
                        vec.push((node_hash, db_state));
                    }
                    Err(_e) => {
                        return Err(Box::new(Exception::new("Poison error")));
                    }
                }
                Ok(Async::Ready(node_ref))
            }
            NodeType::Branch(mut next_node) => {
                let mut removal_vec = Vec::with_capacity(self.futures.len());
                for (index, node) in &mut self.futures.iter_mut().enumerate() {
                    if let Async::Ready(node_ref) = node.poll()? {
                        // Node subtree has finished operation
                        next_node
                            .node_refs
                            .insert(node_ref.node_location[0], node_ref);
                        removal_vec.push(index);
                    }
                }
                while removal_vec.len() > 0 {
                    if let Some(index) = removal_vec.pop() {
                        self.futures.remove(index);
                    }
                }
                if self.futures.len() == 0 {
                    if next_node.node_refs.len() == 1 {
                        let mut iter = next_node.node_refs.iter_mut();
                        match iter.next() {
                            Some((_key, node_ref)) => {
                                self.location.append(&mut node_ref.node_location);
                                let node_ref = NodeRef::new(&self.location, &node_ref.child);
                                return Ok(Async::Ready(node_ref));
                            }
                            None => {
                                return Err(Box::new(Exception::new(
                                    "Couldn't resolve future, node error",
                                )));
                            }
                        }
                    }
                    let node_hash = hash(&next_node.encode()?, 32);
                    let db_state = DBState::new(None, Some(next_node.clone()), 1);
                    let node_ref = NodeRef::new(&self.location, &node_hash);
                    let guard = self.write_queue.lock();
                    match guard {
                        Ok(mut vec) => {
                            vec.push((node_hash, db_state));
                        }
                        Err(_e) => {
                            return Err(Box::new(Exception::new("Poison error")));
                        }
                    }
                    return Ok(Async::Ready(node_ref));
                }
                self.node = NodeType::Branch(next_node);
                Ok(Async::NotReady)
            }
        }
    }
}

impl Debug for TreeNode {
    fn fmt(&self, f: &mut Formatter) -> FormatResult {
        write!(
            f,
            "TreeNode: {{Node: {:?},Location: {:?}, Futures: {:?}, Parent: {:?}}}",
            &self.node, &self.location, &self.futures, &self.parent
        )
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    #[test]
    fn it_awaits_on_an_inner_future() {
        let write_queue = Arc::new(Mutex::new(Vec::new()));
        let node_ref_location = vec![0];
        let node_ref_child = vec![2];
        let node_ref = NodeRef::new(&node_ref_location, &node_ref_child);
        let state_node = StateNode::new(vec![node_ref]);
        let mut root_tree_node = TreeNode::new(
            NodeType::Branch(state_node.clone()),
            vec![0],
            write_queue.clone(),
            0,
        );
        let second_tree_node = TreeNode::new(
            NodeType::Branch(state_node.clone()),
            vec![1],
            write_queue.clone(),
            0,
        );
        root_tree_node.add_future(&second_tree_node);
        let result = root_tree_node.wait();
        match result {
            Ok(_node) => {
                let len = write_queue.lock().unwrap().len();
                assert_eq!(len, 1);
            }
            Err(e) => {
                println!("Error: {:?}", e);
                unimplemented!()
            }
        }
    }
}
