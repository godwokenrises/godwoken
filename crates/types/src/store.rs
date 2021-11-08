use std::convert::TryFrom;

use anyhow::{bail, Result};
use sparse_merkle_tree::merge::MergeValue;
use sparse_merkle_tree::tree::BranchNode;

const MERGE_WITH_ZERO_LEN: usize = 65;
const MERGE_VALUE_LEN: usize = 32;
const MAX_SMT_BRANCH_LEN: usize = 1 + MERGE_WITH_ZERO_LEN * 2; // first is flag

#[repr(u8)]
#[derive(Clone, Copy)]
pub enum SMTMergeValueFlag {
    BothMergeWithZero = 0, // 00
    RightValue = 1,        // 01 LeftMergeWithZero
    LeftValue = 2,         // 10 RightMergeWithZero
    BothValue = 3,         // 11
}

impl TryFrom<u8> for SMTMergeValueFlag {
    type Error = anyhow::Error;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        let flag = match value {
            0 => SMTMergeValueFlag::BothMergeWithZero,
            1 => SMTMergeValueFlag::RightValue,
            2 => SMTMergeValueFlag::LeftValue,
            3 => SMTMergeValueFlag::BothValue,
            _ => bail!("invalid smt branch node flag {}", value),
        };

        Ok(flag)
    }
}

pub struct SMTBranchNode {
    flag: SMTMergeValueFlag,
    len: usize,
    buf: [u8; MAX_SMT_BRANCH_LEN],
}

impl SMTBranchNode {
    pub fn as_slice(&self) -> &[u8] {
        &self.buf[..self.len]
    }

    pub fn uncheck_from_slice(slice: &[u8]) -> Self {
        let flag = SMTMergeValueFlag::try_from(slice[0]).unwrap();
        let len = slice.len();
        let mut buf = [0u8; MAX_SMT_BRANCH_LEN];
        buf[..len].copy_from_slice(&slice[..len]);

        SMTBranchNode { flag, len, buf }
    }

    pub fn from_slice(slice: &[u8]) -> Result<Self> {
        if slice.is_empty() {
            bail!("invalid smt branch node slice");
        }

        let flag = SMTMergeValueFlag::try_from(slice[0])?;
        let len = slice.len();
        match flag {
            SMTMergeValueFlag::BothMergeWithZero if len == MAX_SMT_BRANCH_LEN => {}
            SMTMergeValueFlag::BothValue if len == 1 + MERGE_VALUE_LEN * 2 => {}
            SMTMergeValueFlag::RightValue | SMTMergeValueFlag::LeftValue
                if len == 1 + MERGE_VALUE_LEN + MERGE_WITH_ZERO_LEN => {}
            _ => bail!("invalid smt branch node slice"),
        };

        let mut buf = [0u8; MAX_SMT_BRANCH_LEN];
        buf[..len].copy_from_slice(&slice[..len]);

        Ok(SMTBranchNode { flag, len, buf })
    }
}

impl From<&BranchNode> for SMTBranchNode {
    fn from(branch: &BranchNode) -> Self {
        let flag = match (&branch.left, &branch.right) {
            (MergeValue::MergeWithZero { .. }, MergeValue::MergeWithZero { .. }) => {
                SMTMergeValueFlag::BothMergeWithZero
            }
            (MergeValue::MergeWithZero { .. }, MergeValue::Value(_)) => {
                SMTMergeValueFlag::RightValue
            }
            (MergeValue::Value(_), MergeValue::MergeWithZero { .. }) => {
                SMTMergeValueFlag::LeftValue
            }
            (MergeValue::Value(_), MergeValue::Value(_)) => SMTMergeValueFlag::BothValue,
        };

        let mut buf = [flag as u8; MAX_SMT_BRANCH_LEN];
        let mut len = 1;
        match branch.left {
            MergeValue::Value(byte32) => {
                buf[1..33].copy_from_slice(&byte32.as_slice()[..32]);
                len += 32;
            }
            MergeValue::MergeWithZero {
                base_node,  // byte32
                zero_bits,  // byte32
                zero_count, // u8
            } => {
                buf[1..33].copy_from_slice(&base_node.as_slice()[..32]);
                buf[33..65].copy_from_slice(&zero_bits.as_slice()[..32]);
                buf[65] = zero_count;
                len += 65
            }
        }
        match branch.right {
            MergeValue::Value(byte32) => {
                buf[len..len + 32].copy_from_slice(&byte32.as_slice()[..32]);
                len += 32;
            }
            MergeValue::MergeWithZero {
                base_node,  // byte32
                zero_bits,  // byte32
                zero_count, // u8
            } => {
                buf[len..len + 32].copy_from_slice(&base_node.as_slice()[..32]);
                len += 32;
                buf[len..len + 32].copy_from_slice(&zero_bits.as_slice()[..32]);
                len += 32;
                buf[len] = zero_count;
                len += 1;
            }
        }
        SMTBranchNode { flag, len, buf }
    }
}

impl From<&SMTBranchNode> for BranchNode {
    fn from(branch: &SMTBranchNode) -> Self {
        let parse_merge_with_zero = |bytes: &[u8]| -> MergeValue {
            let mut base_node = [0u8; 32];
            base_node.copy_from_slice(&bytes[0..32]);
            let mut zero_bits = [0u8; 32];
            zero_bits.copy_from_slice(&bytes[32..64]);
            let zero_count = bytes[64];

            MergeValue::MergeWithZero {
                base_node: base_node.into(),
                zero_bits: zero_bits.into(),
                zero_count,
            }
        };

        let parse_value = |bytes: &[u8]| -> MergeValue {
            let mut hash = [0u8; 32];
            hash.copy_from_slice(&bytes[..32]);
            MergeValue::Value(hash.into())
        };

        let branch_buf = &branch.buf[1..]; // trim first flag
        match branch.flag {
            SMTMergeValueFlag::BothMergeWithZero if branch.len == MAX_SMT_BRANCH_LEN => {
                BranchNode {
                    left: parse_merge_with_zero(&branch_buf[0..MERGE_WITH_ZERO_LEN]),
                    right: parse_merge_with_zero(
                        &branch_buf[MERGE_WITH_ZERO_LEN..MERGE_WITH_ZERO_LEN * 2],
                    ),
                }
            }
            SMTMergeValueFlag::RightValue
                if branch.len == 1 + MERGE_WITH_ZERO_LEN + MERGE_VALUE_LEN =>
            {
                BranchNode {
                    left: parse_merge_with_zero(&branch_buf[0..MERGE_WITH_ZERO_LEN]),
                    right: parse_value(
                        &branch_buf[MERGE_WITH_ZERO_LEN..MERGE_WITH_ZERO_LEN + MERGE_VALUE_LEN],
                    ),
                }
            }
            SMTMergeValueFlag::LeftValue
                if branch.len == 1 + MERGE_VALUE_LEN + MERGE_WITH_ZERO_LEN =>
            {
                BranchNode {
                    left: parse_value(&branch_buf[0..MERGE_VALUE_LEN]),
                    right: parse_merge_with_zero(
                        &branch_buf[MERGE_VALUE_LEN..MERGE_VALUE_LEN + MERGE_WITH_ZERO_LEN],
                    ),
                }
            }
            SMTMergeValueFlag::BothValue if branch.len == 1 + MERGE_VALUE_LEN * 2 => BranchNode {
                left: parse_value(&branch_buf[0..MERGE_VALUE_LEN]),
                right: parse_value(&branch_buf[MERGE_VALUE_LEN..MERGE_VALUE_LEN * 2]),
            },
            _ => unreachable!("invalid smt branch node"),
        }
    }
}

#[cfg(test)]
mod tests {
    use sparse_merkle_tree::{merge::MergeValue, tree::BranchNode};

    use crate::store::SMTBranchNode;

    #[test]
    fn test_smt_branch_node() {
        let merge_with_zero = MergeValue::MergeWithZero {
            base_node: [1u8; 32].into(),
            zero_bits: [2u8; 32].into(),
            zero_count: 3,
        };
        let value = MergeValue::Value([4u8; 32].into());

        // Both merge with zero
        let branch = BranchNode {
            left: merge_with_zero.clone(),
            right: merge_with_zero.clone(),
        };
        let smt_branch = SMTBranchNode::from(&branch);
        assert_eq!(branch, BranchNode::from(&smt_branch));
        assert_eq!(
            branch,
            BranchNode::from(&(SMTBranchNode::from_slice(smt_branch.as_slice()).unwrap()))
        );

        // Right value
        let branch = BranchNode {
            left: merge_with_zero.clone(),
            right: value.clone(),
        };
        let smt_branch = SMTBranchNode::from(&branch);
        assert_eq!(branch, BranchNode::from(&smt_branch));
        assert_eq!(
            branch,
            BranchNode::from(&(SMTBranchNode::from_slice(smt_branch.as_slice()).unwrap()))
        );

        // Left value
        let branch = BranchNode {
            left: value.clone(),
            right: merge_with_zero,
        };
        let smt_branch = SMTBranchNode::from(&branch);
        assert_eq!(branch, BranchNode::from(&smt_branch));
        assert_eq!(
            branch,
            BranchNode::from(&(SMTBranchNode::from_slice(smt_branch.as_slice()).unwrap()))
        );

        // Both value
        let branch = BranchNode {
            left: value.clone(),
            right: value,
        };
        let smt_branch = SMTBranchNode::from(&branch);
        assert_eq!(branch, BranchNode::from(&smt_branch));
        assert_eq!(
            branch,
            BranchNode::from(&(SMTBranchNode::from_slice(smt_branch.as_slice()).unwrap()))
        );
    }
}
