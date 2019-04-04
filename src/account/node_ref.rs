use std::error::Error;

use crate::serialization::state::NodeRef as ProtoNodeRef;
use crate::traits::{Decode, Encode, Proto};

use protobuf::Message as ProtoMessage;

#[derive(Clone, Debug, PartialEq)]
pub struct NodeRef {
    pub node_location: Vec<u8>,
    pub child: Vec<u8>,
}

impl NodeRef {
    pub fn new(location: &Vec<u8>, child: &Vec<u8>) -> NodeRef {
        NodeRef {
            node_location: location.to_vec(),
            child: child.to_vec(),
        }
    }
}

impl Decode for NodeRef {
    fn decode(buffer: &[u8]) -> Result<NodeRef, Box<Error>> {
        let mut proto_node_ref = ProtoNodeRef::new();
        proto_node_ref.merge_from_bytes(buffer)?;
        Ok(NodeRef::new(&proto_node_ref.address, &proto_node_ref.child))
    }
}

impl Proto for NodeRef {
    type ProtoType = ProtoNodeRef;
    fn to_proto(&self) -> Result<Self::ProtoType, Box<Error>> {
        let mut proto_node_ref = Self::ProtoType::new();
        proto_node_ref.set_address(self.node_location.clone());
        proto_node_ref.set_child(self.child.clone());
        Ok(proto_node_ref)
    }

    fn from_proto(_prototype: &Self::ProtoType) -> Result<Self, Box<Error>> {
        unimplemented!()
    }
}

impl Encode for NodeRef {
    fn encode(&self) -> Result<Vec<u8>, Box<Error>> {
        let proto_node_ref = self.to_proto()?;
        Ok(proto_node_ref.write_to_bytes()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_makes_a_node_ref() {
        let addr = vec![109, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        let child = vec![137, 28, 167, 193, 135, 226, 96, 56, 197, 123];
        let node_ref = NodeRef::new(&addr, &child);
        assert_eq!(node_ref.node_location, addr);
        assert_eq!(node_ref.child, child);
    }

    #[test]
    fn it_makes_a_node_ref_from_inode_ref() {
        let addr_slice = vec![109, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        let child = vec![137, 28, 167, 193, 135, 226, 96, 56, 197, 123];
        let mut inode_ref = ProtoNodeRef::new();
        inode_ref.set_address(addr_slice);
        inode_ref.set_child(child);
        let encoded = inode_ref.write_to_bytes().unwrap();

        let node_ref: NodeRef = NodeRef::decode(&encoded).unwrap();
        assert_eq!(
            node_ref.node_location,
            [109, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]
        );
        assert_eq!(
            node_ref.child,
            vec![137, 28, 167, 193, 135, 226, 96, 56, 197, 123]
        );
    }

    #[test]
    fn it_encodes_like_javascript_for_non_zero() {
        let addr_slice = vec![109, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        let child = vec![
            137, 28, 167, 193, 135, 226, 96, 56, 197, 123, 221, 237, 249, 5, 134, 194, 38, 184,
            100, 131, 41, 152, 47, 186, 185, 70, 18, 162, 105, 115, 14, 42,
        ];
        let node_ref = NodeRef::new(&addr_slice, &child);
        let encoding = node_ref.encode().unwrap();
        let javascript_encoding = vec![
            10, 20, 109, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 18, 32, 137, 28,
            167, 193, 135, 226, 96, 56, 197, 123, 221, 237, 249, 5, 134, 194, 38, 184, 100, 131,
            41, 152, 47, 186, 185, 70, 18, 162, 105, 115, 14, 42,
        ];

        assert_eq!(encoding, javascript_encoding);
    }

    #[test]
    fn it_encodes_like_javascript_for_zero() {
        let addr_slice = vec![0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        let child = vec![
            0, 28, 0, 193, 0, 226, 0, 56, 0, 123, 0, 237, 0, 5, 0, 194, 0, 184, 0, 131, 0, 152, 0,
            186, 0, 70, 0, 162, 0, 115, 0, 42,
        ];
        let node_ref = NodeRef::new(&addr_slice, &child);
        let encoding = node_ref.encode().unwrap();
        let javascript_encoding = vec![
            10, 20, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 18, 32, 0, 28, 0,
            193, 0, 226, 0, 56, 0, 123, 0, 237, 0, 5, 0, 194, 0, 184, 0, 131, 0, 152, 0, 186, 0,
            70, 0, 162, 0, 115, 0, 42,
        ];

        assert_eq!(encoding, javascript_encoding);
    }
}
