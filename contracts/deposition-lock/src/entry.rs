//! Deposition-lock
//! A user can send a deposition request cell with this lock.
//! The cell can be unlocked by the rollup cell which match the rollup_type_hash,
//! or can be unlocked by user.
//!
//! Args: DepositionLockArgs

// Import from `core` instead of from `std` since we are in no-std mode
use core::result::Result;

// Import heap related library from `alloc`
// https://doc.rust-lang.org/alloc/index.html
// use alloc::{vec, vec::Vec};

// Import CKB syscalls and structures
// https://nervosnetwork.github.io/ckb-std/riscv64imac-unknown-none-elf/doc/ckb_std/index.html
use crate::ckb_std::{
    ckb_constants::Source,
    ckb_types::{bytes::Bytes, prelude::Unpack as CKBTypeUnpack},
    high_level::{load_input_since, load_script},
    since::Since,
};

use validator_utils::{search_owner_cell, search_rollup_cell};

use gw_types::{
    packed::{DepositionLockArgs, DepositionLockArgsReader},
    prelude::*,
};

use crate::error::Error;

fn parse_lock_args() -> Result<DepositionLockArgs, Error> {
    let script = load_script()?;
    let args: Bytes = script.args().unpack();
    match DepositionLockArgsReader::verify(&args, false) {
        Ok(()) => Ok(DepositionLockArgs::new_unchecked(args)),
        Err(_) => Err(Error::InvalidArgs),
    }
}

// We have two unlock paths
// 1. unlock by Rollup cell
// 2. unlock by user after timeout
//
// We always try the 1 first, then try 2, otherwise the unlock return a failure.
pub fn main() -> Result<(), Error> {
    let lock_args = parse_lock_args()?;
    // try unlock by Rollup
    // return success if rollup cell in the inputs, the following verification will be handled by rollup state validator.
    if search_rollup_cell(&lock_args.rollup_type_hash().unpack()).is_some() {
        return Ok(());
    }

    // unlock by user
    // 1. check since is satisfied the cancel timeout
    let input_since = Since::new(load_input_since(0, Source::GroupInput)?);
    let cancel_timeout = Since::new(lock_args.cancel_timeout().unpack());
    if input_since.flags() != cancel_timeout.flags()
        || input_since.as_u64() < cancel_timeout.as_u64()
    {
        return Err(Error::InvalidSince);
    }
    // 2. search owner cell
    match search_owner_cell(&lock_args.owner_lock_hash().unpack()) {
        Some(_) => Ok(()),
        None => Err(Error::OwnerCellNotFound),
    }
}
