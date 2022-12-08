use gw_types::packed::RollupConfig;
use gw_types::prelude::*;

use crate::testing_tool::chain::DEFAULT_FINALITY_BLOCKS;

pub fn default_rollup_config() -> RollupConfig {
    RollupConfig::new_builder()
        .finality_blocks(DEFAULT_FINALITY_BLOCKS.pack())
        .build()
}
