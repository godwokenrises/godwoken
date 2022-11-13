use gw_config::ForkConfig;
use gw_types::core::H256;
use gw_types::{packed::RollupConfig, prelude::Unpack};

/// A wildly used context, contains several common-used configurations.
#[derive(Clone)]
pub struct RollupContext {
    /// rollup_state_cell's type hash
    pub rollup_script_hash: H256,
    pub rollup_config: RollupConfig,
    pub fork_config: ForkConfig,
}

impl RollupContext {
    pub fn last_finalized_block_number(&self, tip_number: u64) -> u64 {
        let finality: u64 = self.rollup_config.finality_blocks().unpack();
        tip_number.saturating_sub(finality)
    }
}
