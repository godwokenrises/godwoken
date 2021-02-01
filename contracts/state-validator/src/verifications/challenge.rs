use gw_common::H256;
use gw_types::{
    core::Status,
    packed::{GlobalState, RollupConfig},
    prelude::*,
};
use validator_utils::{ckb_std::ckb_constants::Source, error::Error};

use super::{check_rollup_lock_cells, check_status};
use crate::cells::find_challenge_cell;

pub fn verify_enter_challenge(
    rollup_type_hash: H256,
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
        return Err(Error::InvalidChallengeCell);
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
        return Err(Error::InvalidChallengeCell);
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
        return Err(Error::InvalidPostGlobalState);
    }
    Ok(())
}
