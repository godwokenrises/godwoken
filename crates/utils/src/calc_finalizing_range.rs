use anyhow::{Context, Result};
use gw_config::ForkConfig;
use gw_store::traits::chain_store::ChainStore;
use gw_types::{
    core::Timepoint,
    offchain::CompatibleFinalizedTimepoint,
    packed::{L2Block, RollupConfig},
    prelude::*,
};
use std::ops::Range;

// Returns true is the block of `older_block_number` is finalized for `compatible_finalized_timepoint`
fn is_older_block_finalized(
    fork_config: &ForkConfig,
    db: &impl ChainStore,
    compatible_finalized_timepoint: &CompatibleFinalizedTimepoint,
    older_block_number: u64,
) -> Result<bool> {
    let older_block_hash = db
        .get_block_hash_by_number(older_block_number)?
        .context("get older block hash")?;
    let older_block = db
        .get_block(&older_block_hash)?
        .context("get older block")?;
    let older_timepoint = if fork_config.use_timestamp_as_timepoint(older_block_number) {
        Timepoint::from_timestamp(older_block.raw().timestamp().unpack())
    } else {
        Timepoint::from_block_number(older_block_number)
    };
    Ok(compatible_finalized_timepoint.is_finalized(&older_timepoint))
}

// Returns the highest block that is finalized for `block`.
fn find_finalized_upper_bound(
    rollup_config: &RollupConfig,
    fork_config: &ForkConfig,
    db: &impl ChainStore,
    block: &L2Block,
) -> Result<u64> {
    let block_number = block.raw().number().unpack();
    let finality_blocks = rollup_config.finality_blocks().unpack();

    // When using block number as timepoint, `block_number - finality_blocks` is the only finalizing
    // block.
    //
    // NOTE: For simplicity, we return `block_number - finality_blocks` as an upper bound, even if
    // some timestamp-as-timepoint blocks are also finalized for `block`.
    // This off-chain trick is safe.
    if !fork_config.use_timestamp_as_timepoint(block_number.saturating_sub(finality_blocks)) {
        return Ok(block_number.saturating_sub(finality_blocks));
    }

    let global_state = db
        .get_block_post_global_state(&block.hash().into())?
        .context("get current block global state")?;
    let compatible_finalized_timepoint =
        CompatibleFinalizedTimepoint::from_global_state(&global_state, finality_blocks);
    let mut l = fork_config
        .upgrade_global_state_version_to_v2
        .context("upgrade_global_state_version_to_v2 configuration required")?;

    // When using timestamp as timepoint, binary search for the last finalized one for
    // `compatible_finalized_timepoint`.

    // NOTE: To ensure that at least one finalized block is found below, start a binary search at
    // `upgrade_global_state_version_to_v2 - 1`.
    l = l.saturating_sub(1);
    let mut r = block.raw().number().unpack().saturating_sub(1);
    while l < r {
        let mid = l + (r - l + 1) / 2;
        if is_older_block_finalized(fork_config, db, &compatible_finalized_timepoint, mid)? {
            l = mid;
        } else {
            r = mid.saturating_sub(1);
        }
    }

    Ok(l)
}

/// Calculates finalizing range for a block.
///
/// "Block _X_ is finalizing for block _Y_" means that they meet the following criteria:
/// - block _X_ is not finalized for block _Y-1_
/// - block _X_ is finalized for block _Y_
pub fn calc_finalizing_range(
    rollup_config: &RollupConfig,
    fork_config: &ForkConfig,
    db: &impl ChainStore,
    current_block: &L2Block,
) -> Result<Range<u64>> {
    if current_block.raw().number().unpack() == 0 {
        return Ok(0..0);
    }

    let parent_hash = current_block.raw().parent_block_hash().unpack();
    let parent_block = db.get_block(&parent_hash)?.context("get parent block")?;
    let parent_finalized_upper_bound =
        find_finalized_upper_bound(rollup_config, fork_config, db, &parent_block)?;
    let current_finalized_upper_bound =
        find_finalized_upper_bound(rollup_config, fork_config, db, current_block)?;

    // `0..=parent_finalized_upper_bound` blocks is finalized for `parent_block`,
    // `0..=current_finalized_upper_bound` blocks is finalized for `current_block`,
    // then `parent_finalized_upper_bound+1..=current_finalized_upper_bound` is finalizing for `current_block`
    Ok(parent_finalized_upper_bound + 1..current_finalized_upper_bound + 1)
}
