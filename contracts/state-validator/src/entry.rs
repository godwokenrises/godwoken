// Import from `core` instead of from `std` since we are in no-std mode
use core::result::Result;

// Import heap related library from `alloc`
// https://doc.rust-lang.org/alloc/index.html
use validator_utils::{ckb_std::high_level::load_cell_capacity, search_cells::parse_rollup_action};

// Import CKB syscalls and structures
// https://nervosnetwork.github.io/ckb-std/riscv64imac-unknown-none-elf/doc/ckb_std/index.html
use crate::{
    cells::{load_rollup_config, parse_global_state},
    ckb_std::{ckb_constants::Source, high_level::load_script_hash},
    verifications,
};

use gw_types::{packed::RollupActionUnion, prelude::*};

use validator_utils::error::Error;

/// return true if we are in the initialization, otherwise return false
fn check_initialization() -> Result<bool, Error> {
    if load_cell_capacity(0, Source::GroupInput).is_ok() {
        return Ok(false);
    }
    // no input Rollup cell, which represents we are in the initialization
    let post_global_state = parse_global_state(Source::GroupOutput)?;
    // check config cell
    let _rollup_config = load_rollup_config(&post_global_state.rollup_config_hash().unpack())?;
    Ok(true)
}

pub fn main() -> Result<(), Error> {
    // return success if we are in the initialization
    if check_initialization()? {
        return Ok(());
    }
    // basic verification
    let prev_global_state = parse_global_state(Source::GroupInput)?;
    let post_global_state = parse_global_state(Source::GroupOutput)?;
    let rollup_config = load_rollup_config(&prev_global_state.rollup_config_hash().unpack())?;
    let rollup_type_hash = load_script_hash()?;
    let action = parse_rollup_action(0, Source::GroupOutput)?;
    match action.to_enum() {
        RollupActionUnion::L2Block(l2block) => {
            // verify submit block
            verifications::submit_block::verify(
                rollup_type_hash,
                &rollup_config,
                &l2block,
                &prev_global_state,
                &post_global_state,
            )?;
        }
        RollupActionUnion::RollupEnterChallenge(_args) => {
            // verify enter challenge
            verifications::challenge::verify_enter_challenge(
                rollup_type_hash,
                &rollup_config,
                &prev_global_state,
                &post_global_state,
            )?;
        }
        RollupActionUnion::RollupCancelChallenge(_args) => {
            // verify cancel challenge
            verifications::challenge::verify_cancel_challenge(
                rollup_type_hash,
                &rollup_config,
                &prev_global_state,
                &post_global_state,
            )?;
        }
        RollupActionUnion::RollupRevert(args) => {
            // verify revert
            verifications::revert::verify(
                rollup_type_hash,
                &rollup_config,
                &prev_global_state,
                &post_global_state,
                args,
            )?;
        }
    }

    Ok(())
}
