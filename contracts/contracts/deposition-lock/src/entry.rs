//! Deposition-lock
//! A user can send a deposition request cell with this lock.
//! The cell can be unlocked by the rollup cell which match the rollup_type_id,
//! or can be unlocked by user.
//!
//! Args: rollup_type_id|pubkey_hash|account_id
//!
//! If the account_id is 0, the Rollup should create a new account which pubkey_hash equals to the pubkey_hash in the args.
//! If the account_id isn't 0, the Rollup should mint new token to the account_id according to the deposited token.

// Import from `core` instead of from `std` since we are in no-std mode
use core::result::Result;

// Import heap related library from `alloc`
// https://doc.rust-lang.org/alloc/index.html
// use alloc::{vec, vec::Vec};

// Import CKB syscalls and structures
// https://nervosnetwork.github.io/ckb-std/riscv64imac-unknown-none-elf/doc/ckb_std/index.html
use ckb_std::{
    ckb_constants::Source,
    ckb_types::{bytes::Bytes, prelude::Unpack as CKBTypeUnpack},
    debug,
    high_level::{load_cell_type_hash, load_script, load_witness_args, load_input_since, QueryIter},
    dynamic_loading::CKBDLContext,
    since::{Since, LockValue},
};

use godwoken_types::{
    packed::{DepositionLockArgs, DepositionLockArgsReader},
    prelude::*,
};

use crate::error::Error;
use crate::secp256k1::Secp256k1Lib;

// User can unlock the cell after the timeout
const USER_UNLOCK_TIMEOUT_BLOCKS: u64 = 10;

fn parse_script_args() -> Result<DepositionLockArgs, Error> {
    let script = load_script()?;
    let args: Bytes = script.args().unpack();
    match DepositionLockArgsReader::verify(&args, false) {
        Ok(()) => Ok(DepositionLockArgs::new_unchecked(args)),
        Err(_) => Err(Error::InvalidArgs),
    }
}

fn search_rollup_id(rollup_id: &[u8; 32]) -> Option<usize> {
    QueryIter::new(load_cell_type_hash, Source::Input)
        .position(|type_hash| type_hash.as_ref() == Some(rollup_id))
}

// We have two unlock path
// 1. unlock by Rollup cell
// 2. unlock by user
//
// We read the witness_args to determine which unlock path we are trying.
// if the length of lock_args field is 0 we try to search a Rollup id in the inputs cell,
// otherwise we try to read the user signature from witness_args then verifies the user lock,
pub fn main() -> Result<(), Error> {
    let deposition_lock = parse_script_args()?;
    debug!("script args is {}", deposition_lock);
    let witness_args = load_witness_args(0, Source::GroupInput)?;
    let lock_args: Bytes = witness_args
        .lock()
        .to_opt().map(|lock| lock.unpack()).unwrap_or_else(|| Bytes::default());
    if lock_args.len() > 0 {
        // unlock by user
        // 1. check since is satisfied the timeout blocks
        let since = Since::new(load_input_since(0, Source::GroupInput)?);
        if !since.is_relative() {
            return Err(Error::InvalidSince);
        }
        match since.extract_lock_value() {
            Some(LockValue::BlockNumber(n)) if n >= USER_UNLOCK_TIMEOUT_BLOCKS => {
                // donothing if the lock value satisfied our requirements.
            }
            _ => {
                return Err(Error::InvalidSince);
            }
        }
        // 2. verify user's signature
        let mut context = CKBDLContext::<[u8; 128 * 1024]>::new();
        let lib = Secp256k1Lib::load(&mut context);
        if !lib.validate_blake2b_sighash_all(&lock_args[..]).map_err(|err_code| {
            debug!("secp256k1 error {}", err_code);
            Error::Secp256k1
        })? {
            return Err(Error::WrongSignature);
        }
    } else {
        // unlock by Rollup
        // Search inputs cells by rollup_type_id
        search_rollup_id(&deposition_lock.rollup_type_id().unpack())
            .ok_or_else(|| Error::RollupCellNotFound)?;
    }

    Ok(())
}
