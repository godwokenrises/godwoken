use gw_config::ForkConfig;
use gw_types::core::Timepoint;
use gw_types::packed::RollupConfig;
use gw_types::prelude::*;

pub fn finalized_timepoint(
    rollup_config: &RollupConfig,
    fork_config: &ForkConfig,
    block_number: u64,
    block_timestamp: u64,
) -> Timepoint {
    if fork_config.use_timestamp_as_timepoint(block_number) {
        // block.timepoint is in new form, represents the future finalized timestamp
        let finality_time_in_ms = rollup_config.finality_time_in_ms();
        Timepoint::from_timestamp(block_timestamp + finality_time_in_ms)
    } else {
        // block.timepoint is in legacy form, represents the its block number
        Timepoint::from_block_number(block_number)
    }
}

pub fn global_state_finalized_timepoint(
    rollup_config: &RollupConfig,
    fork_config: &ForkConfig,
    block_number: u64,
    block_timestamp: u64,
) -> Timepoint {
    if fork_config.use_timestamp_as_timepoint(block_number) {
        // GlobalState.last_finalized_timepoint is in new form, represents the current timestamp
        Timepoint::from_timestamp(block_timestamp)
    } else {
        // GlobalState.last_finalized_timepoint is in legacy form, represents the already finalized block number
        let finality_as_blocks = rollup_config.finality_blocks().unpack();
        Timepoint::from_block_number(block_number.saturating_sub(finality_as_blocks))
    }
}
