use crate::{packed, prelude::*};
use sparse_merkle_tree::tree::{BranchKey, BranchNode};

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
        let left: [u8; 32] = self.left.into();
        let right: [u8; 32] = self.right.into();

        packed::SMTBranchNode::new_builder()
            .left(left.pack())
            .right(right.pack())
            .build()
    }
}

impl<'r> Unpack<BranchNode> for packed::SMTBranchNodeReader<'r> {
    fn unpack(&self) -> BranchNode {
        BranchNode {
            left: self.left().unpack(),
            right: self.right().unpack(),
        }
    }
}
impl_conversion_for_entity_unpack!(BranchNode, SMTBranchNode);

impl Pack<packed::SMTBranchKey> for BranchKey {
    fn pack(&self) -> packed::SMTBranchKey {
        let height = self.height.into();
        let node_key: [u8; 32] = self.node_key.into();

        packed::SMTBranchKey::new_builder()
            .height(height)
            .node_key(node_key.pack())
            .build()
    }
}

impl<'r> Unpack<BranchKey> for packed::SMTBranchKeyReader<'r> {
    fn unpack(&self) -> BranchKey {
        BranchKey {
            height: self.height().into(),
            node_key: self.node_key().unpack(),
        }
    }
}
impl_conversion_for_entity_unpack!(BranchKey, SMTBranchKey);

impl_conversion_for_packed_iterator_pack!(LogItem, LogItemVec);
