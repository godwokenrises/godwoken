use crate::{
    bytes::Bytes,
    packed::{self},
    prelude::*,
};
use sparse_merkle_tree::{
    tree::{BranchNode, LeafNode, NodeType},
    H256 as SMTH256,
};

impl Pack<packed::TransactionKey> for [u8; 36] {
    fn pack(&self) -> packed::TransactionKey {
        packed::TransactionKey::from_slice(&self[..]).expect("impossible: fail to pack [u8; 36]")
    }
}

impl<'r> Unpack<[u8; 36]> for packed::TransactionKeyReader<'r> {
    fn unpack(&self) -> [u8; 36] {
        let ptr = self.as_slice().as_ptr() as *const [u8; 36];
        unsafe { *ptr }
    }
}
impl_conversion_for_entity_unpack!([u8; 36], TransactionKey);

impl Pack<packed::SMTBranchNode> for BranchNode {
    fn pack(&self) -> packed::SMTBranchNode {
        let fork_height = self.fork_height.into();
        let key: [u8; 32] = self.key.into();
        let node_bytes = match self.node_type {
            NodeType::Pair(l, r) => {
                let mut buf = l.as_slice().to_vec();
                buf.extend_from_slice(r.as_slice());
                Bytes::from(buf)
            }
            NodeType::Single(node) => Bytes::from(node.as_slice().to_vec()),
        };
        packed::SMTBranchNode::new_builder()
            .fork_height(fork_height)
            .key(key.pack())
            .node(node_bytes.pack())
            .build()
    }
}

impl<'r> Unpack<BranchNode> for packed::SMTBranchNodeReader<'r> {
    fn unpack(&self) -> BranchNode {
        let fork_height = self.fork_height().into();
        let key: [u8; 32] = self.key().unpack();
        let node_bytes: Bytes = self.node().unpack();
        let node_type = if node_bytes.len() == 32 {
            let mut h: [u8; 32] = [0u8; 32];
            h.copy_from_slice(&node_bytes);
            NodeType::Single(h.into())
        } else if node_bytes.len() == 64 {
            let mut l = [0u8; 32];
            let mut r = [0u8; 32];
            l.copy_from_slice(&node_bytes[..32]);
            r.copy_from_slice(&node_bytes[32..]);
            NodeType::Pair(l.into(), r.into())
        } else {
            panic!("Invalid BranchNode data");
        };
        BranchNode {
            fork_height,
            key: key.into(),
            node_type,
        }
    }
}
impl_conversion_for_entity_unpack!(BranchNode, SMTBranchNode);

impl Pack<packed::SMTLeafNode> for LeafNode<SMTH256> {
    fn pack(&self) -> packed::SMTLeafNode {
        let key: [u8; 32] = self.key.into();
        let value: [u8; 32] = self.value.into();
        packed::SMTLeafNode::new_builder()
            .key(key.pack())
            .value(value.pack())
            .build()
    }
}

impl<'r> Unpack<LeafNode<SMTH256>> for packed::SMTLeafNodeReader<'r> {
    fn unpack(&self) -> LeafNode<SMTH256> {
        let key: [u8; 32] = self.key().unpack();
        let value: [u8; 32] = self.value().unpack();
        LeafNode {
            key: key.into(),
            value: value.into(),
        }
    }
}
impl_conversion_for_entity_unpack!(LeafNode<SMTH256>, SMTLeafNode);

impl_conversion_for_packed_iterator_pack!(LogItem, LogItemVec);
