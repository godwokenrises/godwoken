use sparse_merkle_tree::H256;
use std::ops::RangeInclusive;

use crate::packed::{FinalizingRange, Script};
use crate::prelude::Unpack;

pub struct SudtCustodian {
    pub script_hash: H256,
    pub amount: u128,
    pub script: Script,
}

impl FinalizingRange {
    // Returns `(from_block_number, to_block_number]`
    pub fn range(&self) -> RangeInclusive<u64> {
        let from_block_number = self.from_block_number().unpack();
        let to_block_number = self.to_block_number().unpack();
        from_block_number + 1..=to_block_number
    }
}
