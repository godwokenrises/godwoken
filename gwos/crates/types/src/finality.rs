use crate::packed::RollupConfig;
use crate::prelude::Unpack;

// Rollup_config.finality_blocks on Godwoken mainnet is set to 16800, and it is
// expected to be equal to ~7 days. So we estimate the average block interval
// is
//
//   7 * 24 * 60 * 60 / 16800 * 1000 = 36000 (millsecond)
//
const BLOCK_INTERVAL_IN_MILLISECONDS: u64 = 36000;

impl RollupConfig {
    /// Convert RollupConfig.finality_blocks, currently represented as a block
    /// count, to a duration representation.
    pub fn finality_time_in_ms(&self) -> u64 {
        self.finality_blocks()
            .unpack()
            .saturating_mul(BLOCK_INTERVAL_IN_MILLISECONDS)
    }
}
