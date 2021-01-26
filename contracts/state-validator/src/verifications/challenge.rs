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

use super::{check_rollup_lock_cells, check_status};
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
use core::{convert::TryInto, num};

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
