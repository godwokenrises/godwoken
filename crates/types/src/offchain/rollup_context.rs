use sparse_merkle_tree::H256;

use crate::{packed::RollupConfig, prelude::Unpack};

#[derive(Clone)]
pub struct RollupContext {
    pub rollup_script_hash: H256,
    pub rollup_config: RollupConfig,
}

impl RollupContext {
    pub fn last_finalized_block_number(&self, tip_number: u64) -> u64 {
        let finality: u64 = self.rollup_config.finality_blocks().unpack();
        tip_number.saturating_sub(finality)
    }
}
