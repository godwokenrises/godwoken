use crate::core::Timepoint;
use crate::packed::GlobalState;
use crate::prelude::*;

// Even after Godwoken has upgraded to v2, there are still some entities with
// number-based timepoint on L1. This warpper structure includes number-based and
// timestamp-based finalized timepoints, so it can be used to check finality for
// both two kinds of timepoints.
#[derive(Clone, Debug, Default)]
pub struct CompatibleFinalizedTimepoint {
    finalized_block_number: u64,
    finalized_timestamp: Option<u64>,
}

impl CompatibleFinalizedTimepoint {
    pub fn from_global_state(global_state: &GlobalState, rollup_config_finality: u64) -> Self {
        match Timepoint::from_full_value(global_state.last_finalized_timepoint().unpack()) {
            Timepoint::BlockNumber(finalized_block_number) => Self {
                finalized_block_number,
                finalized_timestamp: None,
            },
            Timepoint::Timestamp(finalized_timestamp) => {
                let global_block_number = global_state.block().count().unpack().saturating_sub(1);
                let finality_as_blocks = rollup_config_finality;
                Self {
                    finalized_block_number: global_block_number.saturating_sub(finality_as_blocks),
                    finalized_timestamp: Some(finalized_timestamp),
                }
            }
        }
    }

    /// Returns true if `timepoint` is finalized.
    pub fn is_finalized(&self, timepoint: &Timepoint) -> bool {
        match timepoint {
            Timepoint::BlockNumber(block_number) => *block_number <= self.finalized_block_number,
            Timepoint::Timestamp(timestamp) => {
                match self.finalized_timestamp {
                    Some(finalized_timestamp) => *timestamp <= finalized_timestamp,
                    None => {
                        // it should never happen
                        false
                    }
                }
            }
        }
    }

    // Test cases use only!
    pub fn from_block_number(block_number: u64, rollup_config_finality: u64) -> Self {
        let finality_as_blocks = rollup_config_finality;
        Self {
            finalized_timestamp: None,
            finalized_block_number: block_number.saturating_sub(finality_as_blocks),
        }
    }
}
