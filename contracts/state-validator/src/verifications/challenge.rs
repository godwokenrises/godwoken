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
    types::StakeCell,
};
use alloc::vec;
use core::convert::TryInto;

// this function ensure transaction doesn't contains any deposition / withdrawal / custodian cells
fn check_rollup_lock_cells(
    rollup_type_hash: &[u8; 32],
    config: &RollupConfig,
) -> Result<(), Error> {
    if !collect_deposition_locks(rollup_type_hash, config, Source::Input)?.is_empty() {
        return Err(Error::Challenge);
    }
    if !collect_deposition_locks(rollup_type_hash, config, Source::Output)?.is_empty() {
        return Err(Error::Challenge);
    }
    if !collect_withdrawal_locks(rollup_type_hash, config, Source::Input)?.is_empty() {
        return Err(Error::Challenge);
    }
    if !collect_withdrawal_locks(rollup_type_hash, config, Source::Output)?.is_empty() {
        return Err(Error::Challenge);
    }
    if !collect_custodian_locks(rollup_type_hash, config, Source::Input)?.is_empty() {
        return Err(Error::Challenge);
    }
    if !collect_custodian_locks(rollup_type_hash, config, Source::Output)?.is_empty() {
        return Err(Error::Challenge);
    }
    Ok(())
}

pub fn verify_enter_challenge(
    rollup_type_hash: [u8; 32],
    config: &RollupConfig,
    prev_global_state: &GlobalState,
    post_global_state: &GlobalState,
) -> Result<(), Error> {
    check_status(prev_global_state, Status::Running)?;
    // check challenge cells
    let has_input_challenge =
        find_challenge_cell(&rollup_type_hash, config, Source::Input)?.is_some();
    let has_output_challenge =
        find_challenge_cell(&rollup_type_hash, config, Source::Output)?.is_some();
    if has_input_challenge || !has_output_challenge {
        return Err(Error::Challenge);
    }
    // reject if contains any stake cell
    if find_stake_cell(&rollup_type_hash, config, Source::Input, None)?.is_some()
        || find_stake_cell(&rollup_type_hash, config, Source::Output, None)?.is_some()
    {
        return Err(Error::Challenge);
    }
    // check rollup lock cells
    check_rollup_lock_cells(&rollup_type_hash, config)?;
    // check post global state
    let actual_post_global_state = {
        let status: u8 = Status::Halting.into();
        prev_global_state
            .clone()
            .as_builder()
            .status(status.into())
            .build()
    };
    if post_global_state != &actual_post_global_state {
        return Err(Error::PostGlobalState);
    }
    Ok(())
}

pub fn verify_cancel_challenge(
    rollup_type_hash: [u8; 32],
    config: &RollupConfig,
    prev_global_state: &GlobalState,
    post_global_state: &GlobalState,
) -> Result<(), Error> {
    check_status(prev_global_state, Status::Halting)?;
    // check challenge cells
    let has_input_challenge =
        find_challenge_cell(&rollup_type_hash, config, Source::Input)?.is_some();
    let has_output_challenge =
        find_challenge_cell(&rollup_type_hash, config, Source::Output)?.is_some();
    if !has_input_challenge || has_output_challenge {
        return Err(Error::Challenge);
    }
    // reject if contains any stake cell
    if find_stake_cell(&rollup_type_hash, config, Source::Input, None)?.is_some()
        || find_stake_cell(&rollup_type_hash, config, Source::Output, None)?.is_some()
    {
        return Err(Error::Challenge);
    }
    // check rollup lock cells
    check_rollup_lock_cells(&rollup_type_hash, config)?;
    // check post global state
    let actual_post_global_state = {
        let status: u8 = Status::Running.into();
        prev_global_state
            .clone()
            .as_builder()
            .status(status.into())
            .build()
    };
    if post_global_state != &actual_post_global_state {
        return Err(Error::PostGlobalState);
    }
    Ok(())
}

/// Check Rollup cell is maturity(on the layer1) since entered the challenge
pub fn check_challenge_maturity(_config: &RollupConfig) -> Result<(), Error> {
    const CHALLENGE_MATURITY_BLOCKS: u64 = 10000;

    let since = Since::new(load_input_since(0, Source::GroupInput)?);
    match since.extract_lock_value() {
        Some(LockValue::BlockNumber(n)) => {
            if since.is_relative() && n > CHALLENGE_MATURITY_BLOCKS {
                return Ok(());
            }
        }

        _ => {}
    }
    Err(Error::InvalidStatus)
}

/// Check challenge rewards
pub fn check_challenge_rewards(
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

/// Verify revert
/// 1. check revert merkle roots
/// 2. check reverted block root
/// 3. check other lock cells
/// 4. check stake cell and rewards
pub fn verify_revert(
    rollup_type_hash: [u8; 32],
    config: &RollupConfig,
    prev_global_state: &GlobalState,
    post_global_state: &GlobalState,
    revert_args: RollupRevert,
) -> Result<(), Error> {
    check_status(prev_global_state, Status::Halting)?;
    // check challenge maturity
    check_challenge_maturity(config)?;
    // check challenge cells
    let challenge_cell =
        find_challenge_cell(&rollup_type_hash, config, Source::Input)?.ok_or(Error::Challenge)?;
    let has_output_challenge =
        find_challenge_cell(&rollup_type_hash, config, Source::Output)?.is_some();
    if has_output_challenge {
        return Err(Error::Challenge);
    }
    // check rollup lock cells,
    // we do not handle the reverting of lock cells in here,
    // instead we handle them in the submitting layer2 block action
    check_rollup_lock_cells(&rollup_type_hash, config)?;
    let target_block = revert_args.target_block();
    let target_block_hash: H256 = target_block.hash().into();
    let target_block_smt_key: H256 =
        RawL2Block::compute_smt_key(target_block.number().unpack()).into();
    let challenge_target = challenge_cell.args.target();
    let challenge_block_hash: H256 = challenge_target.block_hash().unpack();
    if challenge_block_hash != target_block_hash {
        return Err(Error::Challenge);
    }
    // check challenge reward
    check_challenge_rewards(
        &rollup_type_hash,
        config,
        &target_block.stake_cell_owner_lock_hash(),
        challenge_cell.args.rewards_receiver_lock(),
    )?;
    // prove the target block exists in the main chain
    let block_merkle_proof = CompiledMerkleProof(revert_args.block_proof().unpack());
    let is_main_chain_block = block_merkle_proof.verify::<Blake2bHasher>(
        &prev_global_state.block().merkle_root().unpack(),
        vec![(target_block_smt_key, target_block_hash)],
    )?;
    if !is_main_chain_block {
        return Err(Error::Challenge);
    }
    // prove the target block isn't in the prev reverted block root
    let reverted_block_merkle_proof =
        CompiledMerkleProof(revert_args.reverted_block_proof().unpack());
    let is_reverted_block_prev = reverted_block_merkle_proof.verify::<Blake2bHasher>(
        &prev_global_state.reverted_block_root().unpack(),
        vec![(target_block_smt_key, H256::zero())],
    )?;
    if is_reverted_block_prev {
        return Err(Error::Challenge);
    }
    // prove the target block in the post reverted block root
    let is_reverted_block_post = reverted_block_merkle_proof.verify::<Blake2bHasher>(
        &post_global_state.reverted_block_root().unpack(),
        vec![(target_block_smt_key, target_block_hash)],
    )?;
    if !is_reverted_block_post {
        return Err(Error::Challenge);
    }
    let reverted_block_root = post_global_state.reverted_block_root();
    // calculate the prev block merkle state (not include the target block)
    let block_merkle_state = {
        let block_root = block_merkle_proof
            .compute_root::<Blake2bHasher>(vec![(target_block_smt_key, H256::zero())])?;
        let block_count = target_block.number();
        BlockMerkleState::new_builder()
            .merkle_root(block_root.pack())
            .count(block_count)
            .build()
    };
    let account_merkle_state = target_block.prev_account();
    let target_block_number: u64 = target_block.number().unpack();
    let last_finalized_block_number = target_block_number
        .saturating_sub(1)
        .saturating_sub(FINALIZE_BLOCKS);
    // check post global state
    let actual_post_global_state = {
        let status: u8 = Status::Running.into();
        prev_global_state
            .clone()
            .as_builder()
            .account(account_merkle_state)
            .block(block_merkle_state)
            .last_finalized_block_number(last_finalized_block_number.pack())
            .reverted_block_root(reverted_block_root)
            .status(status.into())
            .build()
    };
    if post_global_state != &actual_post_global_state {
        return Err(Error::PostGlobalState);
    }
    Ok(())
}
