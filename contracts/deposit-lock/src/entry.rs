//! Deposit-lock
//! A user can send a deposit request cell with this lock.
//! The cell can be unlocked by the rollup cell which match the rollup_type_hash,
//! or can be unlocked by user.
//!
//! Args: DepositLockArgs

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

use gw_utils::cells::{rollup::search_rollup_cell, utils::search_lock_hash};

use gw_types::{
    packed::{DepositLockArgs, DepositLockArgsReader},
    prelude::*,
};
use gw_utils::gw_types;

use crate::error::Error;

/// args: rollup_type_hash | deposit lock args
fn parse_lock_args() -> Result<([u8; 32], DepositLockArgs), Error> {
    let mut rollup_type_hash = [0u8; 32];
    let script = load_script()?;
    let args: Bytes = script.args().unpack();
    if args.len() < rollup_type_hash.len() {
        return Err(Error::InvalidArgs);
    }
    rollup_type_hash.copy_from_slice(&args[..32]);
    match DepositLockArgsReader::verify(&args.slice(32..), false) {
        Ok(()) => Ok((
            rollup_type_hash,
            DepositLockArgs::new_unchecked(args.slice(32..)),
        )),
        Err(_) => Err(Error::InvalidArgs),
    }
}

// We have two unlock paths
// 1. unlock by Rollup cell
// 2. unlock by user after timeout
//
// We always try the 1 first, then try 2, otherwise the unlock return a failure.
pub fn main() -> Result<(), Error> {
    let (rollup_type_hash, lock_args) = parse_lock_args()?;
    // try unlock by Rollup
    // return success if rollup cell in the inputs, the following verification will be handled by rollup state validator.
    if search_rollup_cell(&rollup_type_hash, Source::Input).is_some() {
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
    match search_lock_hash(&lock_args.owner_lock_hash().unpack(), Source::Input) {
        Some(_) => Ok(()),
        None => Err(Error::OwnerCellNotFound),
    }
}
