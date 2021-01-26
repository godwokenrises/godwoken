use gw_common::{
    smt::{Blake2bHasher, CompiledMerkleProof},
    FINALIZE_BLOCKS, H256,
};
use gw_types::{
    core::Status,
    packed::{
        BlockMerkleState, Byte32, ChallengeTarget, GlobalState, RawL2Block, RollupConfig,
        RollupRevert, Script,
    },
    prelude::*,
};
use validator_utils::{
    ckb_std::{
        ckb_constants::Source,
        high_level::load_input_since,
        since::{LockValue, Since},
    },
    search_cells::search_lock_hash,
};

use super::check_status;
use crate::{
    cells::{
        collect_burn_cells, collect_custodian_locks, collect_deposition_locks,
        collect_withdrawal_locks, fetch_capacity_and_sudt_value, find_challenge_cell,
        find_stake_cell,
    },
    error::Error,
    types::{ChallengeCell, StakeCell},
};
use alloc::{vec, vec::Vec};
use core::convert::TryInto;

/// Check challenge rewards
fn check_challenge_rewards(
    rollup_type_hash: &[u8; 32],
    config: &RollupConfig,
    stake_cell_owner_lock_hash: &Byte32,
    reward_receiver_lock: Script,
) -> Result<(), Error> {
    const REWARDS_RATE: u64 = 50;

    // check input stake cell exists
    let stake_cell = find_stake_cell(
        rollup_type_hash,
        config,
        Source::Input,
        Some(stake_cell_owner_lock_hash),
    )?
    .ok_or(Error::Challenge)?;
    // calcuate rewards assets & burn assets
    let challenge_cell = find_challenge_cell(rollup_type_hash, config, Source::Input)?
        .ok_or(Error::InvalidStatus)?;
    let expected_reward_capacity = stake_cell.value.capacity.saturating_mul(REWARDS_RATE) / 100;
    let expected_burn_capacity = stake_cell
        .value
        .capacity
        .saturating_sub(expected_reward_capacity);
    // make sure rewards assets are sent to the challenger
    let reward_receiver_lock_hash = reward_receiver_lock.hash();
    let reward_cell_value = {
        let index =
            search_lock_hash(&reward_receiver_lock_hash, Source::Output).ok_or(Error::Challenge)?;
        fetch_capacity_and_sudt_value(config, index, Source::Output)?
    };
    if reward_cell_value.capacity
        < expected_reward_capacity.saturating_add(challenge_cell.value.capacity)
    {
        return Err(Error::InvalidStatus);
    }
    // make sure burn assets are burned
    let burned_assets = collect_burn_cells(rollup_type_hash, config, Source::Output)?;
    let burned_capacity: u64 = burned_assets.into_iter().map(|c| c.value.capacity).sum();
    if burned_capacity < expected_burn_capacity {
        return Err(Error::InvalidStatus);
    }
    Ok(())
}

pub fn verify(
    rollup_type_hash: [u8; 32],
    config: &RollupConfig,
    prev_global_state: &GlobalState,
    post_global_state: &GlobalState,
) -> Result<(), Error> {
    Ok(())
}
