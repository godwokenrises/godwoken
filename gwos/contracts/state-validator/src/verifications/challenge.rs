use ckb_smt::smt::{Pair, Tree};
use core::convert::TryInto;
use gw_utils::finality::{finality_time_in_ms, is_finalized};
use gw_utils::fork::Fork;
use gw_utils::{
    cells::lock_cells::{collect_burn_cells, find_challenge_cell},
    ckb_std::{ckb_constants::Source, debug},
    error::Error,
};
use gw_utils::{cells::types::ChallengeCell};
use gw_utils::{
    gw_types::{
        core::{ChallengeTargetType, Status, Timepoint, H256},
        packed::{GlobalState, RawL2Block, RollupConfig, RollupEnterChallengeReader},
        prelude::*,
    },
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
    let post_version: u8 = post_global_state.version().into();
    let block_timepoint = if Fork::use_timestamp_as_timepoint(post_version) {
        // new form, represents the future finalized timestamp
        Timepoint::from_timestamp(
            challenged_block.timestamp().unpack() + finality_time_in_ms(config),
        )
    } else {
        // legacy form, represents the current block number
        Timepoint::from_block_number(challenged_block.number().unpack())
    };
    let is_block_finalized = is_finalized(config, post_global_state, &block_timepoint);
    if is_block_finalized {
        debug!("cannot challenge a finalized block");
        return Err(Error::InvalidChallengeTarget);
    }

    // merkle proof
    {
        let mut tree_buf = [Pair::default(); 1];
        let mut smt_tree = Tree::new(&mut tree_buf);
        let key = RawL2Block::compute_smt_key(challenged_block.number().unpack()).into();
        let block_hash = challenged_block.hash().into();
        smt_tree.update(&key, &block_hash).map_err(|err| {
            debug!("[verify_enter_challenge] update smt tree error: {}", err);
            Error::MerkleProof
        })?;

        let root = prev_global_state.block().merkle_root().unpack();
        let proof = witness.block_proof().raw_data();
        smt_tree.verify(&root, &proof).map_err(|err| {
            debug!(
                "[verify_enter_challenge] verify merkle proof error: {}",
                err
            );
            Error::MerkleProof
        })?;
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
