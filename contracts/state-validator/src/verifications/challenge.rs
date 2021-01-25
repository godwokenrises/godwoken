use gw_types::{
    core::Status,
    packed::{GlobalState, RollupConfig},
    prelude::*,
};
use validator_utils::ckb_std::ckb_constants::Source;

use super::check_status;
use crate::{
    cells::{
        collect_custodian_locks, collect_deposition_locks, collect_withdrawal_locks,
        find_challenge_cell, find_stake_cell,
    },
    error::Error,
};
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
    check_status(prev_global_state, Status::Running)?;
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

/// Verify revert
/// 1. check revert merkle roots
/// 2. check reverted block root
/// 3. check other lock cells
/// 4. check stake cell and rewards
/// TODO
pub fn verify_revert(
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
    // check stake cell / deposition / withdrawal / custodian
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
