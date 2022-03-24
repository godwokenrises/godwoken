use std::convert::TryInto;

use crate::{packed, prelude::*};
use molecule::prelude::Byte;
use sparse_merkle_tree::merge::MergeValue;
use sparse_merkle_tree::tree::{BranchKey, BranchNode};

impl Pack<packed::TransactionKey> for [u8; 36] {
    fn pack(&self) -> packed::TransactionKey {
        packed::TransactionKey::from_slice(&self[..]).expect("impossible: fail to pack [u8; 36]")
    }
}

impl<'r> Unpack<[u8; 36]> for packed::TransactionKeyReader<'r> {
    #[inline]
    fn unpack(&self) -> [u8; 36] {
        self.as_slice()
            .try_into()
            .expect("unpack TransactionKeyReader")
    }
}
impl_conversion_for_entity_unpack!([u8; 36], TransactionKey);

impl Pack<packed::WithdrawalKey> for [u8; 36] {
    fn pack(&self) -> packed::WithdrawalKey {
        packed::WithdrawalKey::from_slice(&self[..]).expect("impossible: fail to pack [u8; 36]")
    }
}

impl<'r> Unpack<[u8; 36]> for packed::WithdrawalKeyReader<'r> {
    #[inline]
    fn unpack(&self) -> [u8; 36] {
        self.as_slice()
            .try_into()
            .expect("unpack WithdrawalKeyReader")
    }
}
impl_conversion_for_entity_unpack!([u8; 36], WithdrawalKey);

impl Pack<packed::SMTMergeValue> for MergeValue {
    fn pack(&self) -> packed::SMTMergeValue {
        match self {
            MergeValue::Value(value) => {
                let smt_value = packed::SMTValue::new_builder()
                    .value(Into::<[u8; 32]>::into(*value).pack())
                    .build();

                packed::SMTMergeValue::new_builder()
                    .set(packed::SMTMergeValueUnion::SMTValue(smt_value))
                    .build()
            }
            MergeValue::MergeWithZero {
                base_node,
                zero_bits,
                zero_count,
            } => {
                let merge_with_zero = packed::SMTMergeWithZero::new_builder()
                    .base_node(Into::<[u8; 32]>::into(*base_node).pack())
                    .zero_bits(Into::<[u8; 32]>::into(*zero_bits).pack())
                    .zero_count(Into::<Byte>::into(*zero_count))
                    .build();

                packed::SMTMergeValue::new_builder()
                    .set(packed::SMTMergeValueUnion::SMTMergeWithZero(
                        merge_with_zero,
                    ))
                    .build()
            }
        }
    }
}
impl<'r> Unpack<MergeValue> for packed::SMTMergeValueReader<'r> {
    fn unpack(&self) -> MergeValue {
        match self.to_enum() {
            packed::SMTMergeValueUnionReader::SMTValue(smt_value) => {
                MergeValue::Value(smt_value.value().unpack())
            }
            packed::SMTMergeValueUnionReader::SMTMergeWithZero(merge_with_zero) => {
                MergeValue::MergeWithZero {
                    base_node: merge_with_zero.base_node().unpack(),
                    zero_bits: merge_with_zero.zero_bits().unpack(),
                    zero_count: merge_with_zero.zero_count().into(),
                }
            }
        }
    }
}
impl_conversion_for_entity_unpack!(MergeValue, SMTMergeValue);

impl Pack<packed::SMTBranchNode> for BranchNode {
    fn pack(&self) -> packed::SMTBranchNode {
        packed::SMTBranchNode::new_builder()
            .left(self.left.pack())
            .right(self.right.pack())
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
