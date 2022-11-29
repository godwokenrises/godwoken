//! # How to check finality
//!
//! To determine a block-number-based timepoint is finalized, compare it with
//! `prev_global_state.block.count - 1 + FINALITY_REQUIREMENT`.
//!
//! To determine a timestamp-based timepoint is finalized,
//!   - If prev_global_state.last_finalized_block_number is also timestamp-based,
//!     compare them directly;
//!   - Otherwise, we know it is switching versions, so the corresponding entity
//!     is surely not finalized.

use crate::Timepoint;
use ckb_std::debug;
use gw_types::packed::{GlobalState, RollupConfig};
use gw_types::prelude::Unpack;

// 7 * 24 * 60 * 60 / 16800 * 1000 = 36000
const BLOCK_INTERVAL_IN_MILLISECONDS: u64 = 36000;

pub fn is_finalized(
    rollup_config: &RollupConfig,
    prev_global_state: &GlobalState,
    timepoint: &Timepoint,
) -> bool {
    match timepoint {
        Timepoint::BlockNumber(block_number) => {
            is_block_number_finalized(rollup_config, prev_global_state, *block_number)
        }
        Timepoint::Timestamp(timestamp) => is_timestamp_finalized(prev_global_state, *timestamp),
    }
}

fn is_timestamp_finalized(prev_global_state: &GlobalState, timestamp: u64) -> bool {
    match Timepoint::from_full_value(prev_global_state.last_finalized_block_number().unpack()) {
        Timepoint::BlockNumber(_) => {
            debug!("[is_timestamp_finalized] switching version, prev_global_state.last_finalized_block_number is number-based");
            false
        }
        Timepoint::Timestamp(finalized) => {
            let ret = timestamp <= finalized;
            debug!(
                "[is_timestamp_finalized] is_finalized: {}, prev_global_state last_finalized_timestamp: {}, timestamp: {}",
                ret, finalized, timestamp
            );
            ret
        }
    }
}

fn is_block_number_finalized(
    rollup_config: &RollupConfig,
    prev_global_state: &GlobalState,
    block_number: u64,
) -> bool {
    let finality_blocks: u64 = rollup_config.finality_blocks().unpack();
    let tip_number: u64 = prev_global_state.block().count().unpack().saturating_sub(1);
    let ret = block_number.saturating_add(finality_blocks) <= tip_number;
    debug!(
        "[is_block_number_finalized] is_finalized: {}, prev_global_state tip number: {}, block_number: {}",
        ret, tip_number, block_number
    );
    ret
}

pub fn finality_time_in_ms(rollup_config: &RollupConfig) -> u64 {
    let finality_blocks = rollup_config.finality_blocks().unpack();
    finality_blocks.saturating_mul(BLOCK_INTERVAL_IN_MILLISECONDS)
}
