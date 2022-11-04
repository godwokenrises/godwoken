// Import from `core` instead of from `std` since we are in no-std mode
use core::result::Result;

// Import heap related library from `alloc`
// https://doc.rust-lang.org/alloc/index.html
use gw_utils::{
    cells::rollup::{load_rollup_config, parse_rollup_action, MAX_ROLLUP_WITNESS_SIZE},
    ckb_std::{
        ckb_types::prelude::Unpack as CKBUnpack,
        debug,
        high_level::{load_cell_capacity, load_cell_data, load_script},
    },
    gw_types::packed::{GlobalStateV0, GlobalStateV0Reader, RollupActionUnionReader},
    type_id::{check_type_id, TYPE_ID_SIZE},
};

// Import CKB syscalls and structures
// https://nervosnetwork.github.io/ckb-std/riscv64imac-unknown-none-elf/doc/ckb_std/index.html
use crate::{
    ckb_std::{ckb_constants::Source, high_level::load_script_hash},
    verifications,
};

use gw_types::{
    bytes::Bytes,
    packed::{GlobalState, GlobalStateReader},
    prelude::*,
};
use gw_utils::gw_types;

use gw_utils::error::Error;

const MAX_ROLLUP_VERSION: u8 = 1;

pub fn parse_global_state(source: Source) -> Result<GlobalState, Error> {
    let data = load_cell_data(0, source)?;
    match GlobalStateReader::verify(&data, false) {
        Ok(_) => Ok(GlobalState::new_unchecked(data.into())),
        Err(_) if GlobalStateV0Reader::verify(&data, false).is_ok() => {
            let global_state_v0 = GlobalStateV0::new_unchecked(data.into());
            Ok(GlobalState::from(global_state_v0))
        }
        Err(_) => {
            debug!("Fail to parsing global state");
            Err(Error::Encoding)
        }
    }
}

/// return true if we are in the initialization, otherwise return false
fn check_initialization() -> Result<bool, Error> {
    if load_cell_capacity(0, Source::GroupInput).is_ok() {
        return Ok(false);
    }
    // no input Rollup cell, which represents we are in the initialization
    let post_global_state = parse_global_state(Source::GroupOutput)?;
    // check config cell exists
    let _rollup_config = load_rollup_config(&post_global_state.rollup_config_hash().unpack())?;
    Ok(true)
}

pub fn main() -> Result<(), Error> {
    // check type_id
    {
        let script = load_script()?;
        let args: Bytes = CKBUnpack::unpack(&script.args());
        if args.len() < TYPE_ID_SIZE {
            return Err(Error::InvalidTypeID);
        }
        let mut type_id = [0u8; TYPE_ID_SIZE];
        type_id.copy_from_slice(&args[..TYPE_ID_SIZE]);
        check_type_id(type_id)?;
    }
    // return success if we are in the initialization
    if check_initialization()? {
        return Ok(());
    }
    // basic verification
    let prev_global_state = parse_global_state(Source::GroupInput)?;
    let post_global_state = parse_global_state(Source::GroupOutput)?;
    let rollup_config = load_rollup_config(&prev_global_state.rollup_config_hash().unpack())?;
    let rollup_type_hash = load_script_hash()?.into();

    let post_version: u8 = post_global_state.version().into();
    if post_version > MAX_ROLLUP_VERSION {
        debug!("exceeded max rollup version");
        return Err(Error::InvalidPostGlobalState);
    }
    if post_version < prev_global_state.version().into() {
        debug!("downgrade rollup version");
        return Err(Error::InvalidPostGlobalState);
    }

    // load rollup action
    let mut rollup_witness_buf = [0u8; MAX_ROLLUP_WITNESS_SIZE];
    let action = parse_rollup_action(&mut rollup_witness_buf, 0, Source::GroupOutput)?;
    match action.to_enum() {
        RollupActionUnionReader::RollupSubmitBlock(args) => {
            // verify submit block
            verifications::submit_block::verify(
                rollup_type_hash,
                &rollup_config,
                &args.block(),
                &prev_global_state,
                &post_global_state,
            )?;
            // merkle verify reverted_block_hashes,
            // other rollup locks will check reverted blocks by compare block hash with this field
            verifications::submit_block::verify_reverted_block_hashes(
                args.reverted_block_hashes().unpack(),
                args.reverted_block_proof().unpack(),
                &prev_global_state,
            )?;
        }
        RollupActionUnionReader::RollupEnterChallenge(args) => {
            // verify enter challenge
            verifications::challenge::verify_enter_challenge(
                rollup_type_hash,
                &rollup_config,
                args,
                &prev_global_state,
                &post_global_state,
            )?;
        }
        RollupActionUnionReader::RollupCancelChallenge(_args) => {
            // verify cancel challenge
            verifications::challenge::verify_cancel_challenge(
                rollup_type_hash,
                &rollup_config,
                &prev_global_state,
                &post_global_state,
            )?;
        }
        RollupActionUnionReader::RollupRevert(args) => {
            // verify revert
            verifications::revert::verify(
                rollup_type_hash,
                &rollup_config,
                args,
                &prev_global_state,
                &post_global_state,
            )?;
        }
    }

    Ok(())
}
