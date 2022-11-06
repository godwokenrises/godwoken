use anyhow::Result;
use gw_store::{traits::chain_store::ChainStore, transaction::StoreTransaction};
use gw_types::{
    core::Timepoint,
    offchain::CompatibleFinalizedTimepoint,
    packed::{FinalizingRange, L2Block, RollupConfig},
    prelude::*,
};

/// Calculates FinalizingRange for a block.
///
/// "Block _X_ is finalizing for block _Y_" means that they meet the following criteria:
/// - block _X_ is not finalized for block _Y-1_
/// - block _X_ is finalized for block _Y_
///
/// FinalizingRange represents a range of finalizing block numbers, in the form of (from_block_number, to_block_number]:
///   - when from_block_number < to_block_number, blocks {from_block_number+1, from_block_number+2, ..., to_block_number} are finalizing;
///   - when from_block_number = to_block_number, no any blocks are finalizing
pub fn calc_finalizing_range(
    rollup_config: &RollupConfig,
    db: &StoreTransaction,
    current_block: &L2Block,
) -> Result<FinalizingRange> {
    if current_block.raw().number().unpack() == 0 {
        return Ok(Default::default());
    }

    // Construct CompatibleFinalizedTimepoint of `current_block`, used to check finality of past
    // blocks, or says, filter the finalizing blocks
    let current_block_hash = current_block.hash();
    let current_block_number: u64 = current_block.raw().number().unpack();
    let current_global_state = db
        .get_block_post_global_state(&current_block_hash.into())?
        .expect("get current block global state");
    let compatible_finalized_timepoint = CompatibleFinalizedTimepoint::from_global_state(
        &current_global_state,
        rollup_config.finality_blocks().unpack(),
    );

    // Initialize finalizing range, `(from, to] = (parent_to, parent_to]`;
    let parent_finalizing_range = if current_block_number <= 1 {
        Default::default()
    } else {
        let parent_hash = current_block.raw().parent_block_hash();
        db.get_block_finalizing_range(&parent_hash.unpack())
            .expect("get parent block finalizing range")
    };
    let from_number: u64 = parent_finalizing_range.to_block_number().unpack();
    let mut to_number = from_number;

    // Iterate to determine the finalizing range for the current block
    while to_number + 1 < current_block_number {
        let older_block_number = to_number + 1;
        let older_block_hash = db
            .get_block_hash_by_number(older_block_number)?
            .expect("get finalizing block hash");
        let older_global_state = db
            .get_block_post_global_state(&older_block_hash)?
            .expect("get finalizing block global state");

        // NOTE: It is determined which finality rule to apply based on the global_state.version of
        // older block, but not the version of the current block.
        let older_global_state_version: u8 = older_global_state.version().into();
        let older_timepoint = if older_global_state_version < 2 {
            Timepoint::from_block_number(older_block_number)
        } else {
            // We know global_state.tip_block_timestamp is equal to l2block.timestamp
            Timepoint::from_timestamp(older_global_state.tip_block_timestamp().unpack())
        };
        if !compatible_finalized_timepoint.is_finalized(&older_timepoint) {
            break;
        }

        // This `order_block` just went from unfinalized to finalized for `current_block`.
        // Iterate the next order block.
        to_number += 1;
    }

    Ok(FinalizingRange::new_builder()
        .from_block_number(from_number.pack())
        .to_block_number(to_number.pack())
        .build())
}
