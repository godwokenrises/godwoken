use alloc::vec;
use core::convert::TryInto;
use gw_common::{smt::Blake2bHasher, sparse_merkle_tree::CompiledMerkleProof, H256};
use gw_types::{
    core::{ChallengeTargetType, Status},
    packed::{GlobalState, RollupConfig},
    prelude::*,
};
use gw_utils::{
    cells::lock_cells::{collect_burn_cells, find_challenge_cell},
    ckb_std::{ckb_constants::Source, debug},
    error::Error,
};
use gw_utils::{cells::types::ChallengeCell, gw_types};
use gw_utils::{
    gw_common,
    gw_types::packed::{RawL2Block, RollupEnterChallengeReader},
};

use super::{check_rollup_lock_cells, check_status};

pub fn verify_enter_challenge(
    rollup_type_hash: H256,
    config: &RollupConfig,
    args: RollupEnterChallengeReader,
    prev_global_state: &GlobalState,
    post_global_state: &GlobalState,
) -> Result<(), Error> {
    check_status(prev_global_state, Status::Running)?;
    // check challenge cells
    let has_input_challenge =
        find_challenge_cell(&rollup_type_hash, config, Source::Input)?.is_some();
    if has_input_challenge {
        return Err(Error::InvalidChallengeCell);
    }
    let challenge_cell = find_challenge_cell(&rollup_type_hash, config, Source::Output)?
        .ok_or(Error::InvalidChallengeCell)?;
    // check that challenge target is exists
    let witness = args.witness();
    let challenged_block = witness.raw_l2block();
    // check challenged block isn't finazlied
    if prev_global_state.last_finalized_block_number().unpack()
        >= challenged_block.number().unpack()
    {
        debug!("enter challenge finalized block error");
        return Err(Error::InvalidChallengeTarget);
    }
    let valid = {
        let merkle_proof = CompiledMerkleProof(witness.block_proof().unpack());
        let leaves = vec![(
            RawL2Block::compute_smt_key(challenged_block.number().unpack()).into(),
            challenged_block.hash().into(),
        )];
        merkle_proof
            .verify::<Blake2bHasher>(&prev_global_state.block().merkle_root().unpack(), leaves)?
    };
    if !valid {
        debug!("enter challenge prev state merkle proof error");
        return Err(Error::MerkleProof);
    }
    let challenge_target = challenge_cell.args.target();
    let challenged_block_hash: [u8; 32] = challenge_target.block_hash().unpack();
    if challenged_block.hash() != challenged_block_hash {
        return Err(Error::InvalidChallengeTarget);
    }
    let target_type: ChallengeTargetType = challenge_target
        .target_type()
        .try_into()
        .map_err(|_| Error::InvalidChallengeTarget)?;
    let target_index: u32 = challenge_target.target_index().unpack();
    match target_type {
        ChallengeTargetType::TxExecution | ChallengeTargetType::TxSignature => {
            let tx_count: u32 = challenged_block.submit_transactions().tx_count().unpack();
            if target_index >= tx_count {
                return Err(Error::InvalidChallengeTarget);
            }
        }
        ChallengeTargetType::Withdrawal => {
            let withdrawal_count: u32 = challenged_block
                .submit_withdrawals()
                .withdrawal_count()
                .unpack();
            if target_index >= withdrawal_count {
                return Err(Error::InvalidChallengeTarget);
            }
        }
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
        return Err(Error::InvalidPostGlobalState);
    }
    Ok(())
}

pub fn verify_cancel_challenge(
    rollup_type_hash: H256,
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
        debug!("cancel challenge, invalid challenge cell");
        return Err(Error::InvalidChallengeCell);
    }

    // Check cancel burn
    let challenge_cell = find_challenge_cell(&rollup_type_hash, config, Source::Input)?
        .ok_or(Error::InvalidChallengeCell)?;
    check_cancel_burn(config, &challenge_cell)?;

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
        debug!("cancel challenge, mismatch post global state");
        return Err(Error::InvalidPostGlobalState);
    }
    Ok(())
}

fn check_cancel_burn(config: &RollupConfig, challenge_cell: &ChallengeCell) -> Result<(), Error> {
    let reward_burn_rate: u8 = config.reward_burn_rate().into();
    let challenge_capacity = challenge_cell.value.capacity as u128;
    let expected_burn_capacity = challenge_capacity.saturating_mul(reward_burn_rate.into()) / 100;

    let burned_capacity: u128 = {
        let input_burned_capacity: u128 = collect_burn_cells(config, Source::Input)?
            .into_iter()
            .map(|c| c.value.capacity as u128)
            .sum();
        let output_burned_capacity: u128 = collect_burn_cells(config, Source::Output)?
            .into_iter()
            .map(|c| c.value.capacity as u128)
            .sum();
        output_burned_capacity.saturating_sub(input_burned_capacity)
    };
    if burned_capacity < expected_burn_capacity {
        return Err(Error::InvalidChallengeReward);
    }

    Ok(())
}
