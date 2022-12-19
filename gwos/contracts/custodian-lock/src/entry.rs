// Import from `core` instead of from `std` since we are in no-std mode
use core::result::Result;

use gw_utils::{
    cells::{
        rollup::{
            load_rollup_config, parse_rollup_action, search_rollup_cell, search_rollup_state,
            MAX_ROLLUP_WITNESS_SIZE,
        },
        utils::search_lock_hash,
    },
    ckb_std::high_level::load_cell_lock,
    gw_types::packed::{DepositLockArgs, DepositLockArgsReader, RollupActionUnionReader},
};

// Import CKB syscalls and structures
// https://nervosnetwork.github.io/ckb-std/riscv64imac-unknown-none-elf/doc/ckb_std/index.html
use crate::ckb_std::{
    ckb_constants::Source, ckb_types::bytes::Bytes, ckb_types::prelude::Unpack as CKBUnpack,
    high_level::load_script, high_level::load_witness_args,
};
use gw_types::{
    core::{ScriptHashType, Timepoint},
    packed::{
        CustodianLockArgs, CustodianLockArgsReader, UnlockCustodianViaRevertWitness,
        UnlockCustodianViaRevertWitnessReader,
    },
    prelude::*,
};
use gw_utils::finality::is_finalized;
use gw_utils::gw_types;

use crate::error::Error;

/// args: rollup_type_hash | custodian lock args
fn parse_lock_args() -> Result<([u8; 32], CustodianLockArgs), Error> {
    let script = load_script()?;
    let args: Bytes = script.args().unpack();

    let mut rollup_type_hash: [u8; 32] = [0u8; 32];
    if args.len() < rollup_type_hash.len() {
        return Err(Error::InvalidArgs);
    }
    rollup_type_hash.copy_from_slice(&args[..32]);
    match CustodianLockArgsReader::verify(&args.slice(32..), false) {
        Ok(()) => Ok((
            rollup_type_hash,
            CustodianLockArgs::new_unchecked(args.slice(32..)),
        )),
        Err(_) => Err(Error::InvalidArgs),
    }
}

pub fn main() -> Result<(), Error> {
    let (rollup_type_hash, lock_args) = parse_lock_args()?;

    // read global state from rollup cell
    let global_state = match search_rollup_state(&rollup_type_hash, Source::Input)? {
        Some(state) => state,
        None => return Err(Error::RollupCellNotFound),
    };
    let config = load_rollup_config(&global_state.rollup_config_hash().unpack())?;

    let is_finalized = is_finalized(
        &config,
        &global_state,
        &Timepoint::from_full_value(lock_args.deposit_finalized_timepoint().unpack()),
    );
    if is_finalized {
        // this custodian lock is already finalized, rollup will handle the logic
        return Ok(());
    }

    // otherwise, the submitter try to prove the deposit is reverted.
    let config = load_rollup_config(&global_state.rollup_config_hash().unpack())?;

    // read the args
    let witness_args = load_witness_args(0, Source::GroupInput)?;
    let data: Bytes = witness_args
        .lock()
        .to_opt()
        .ok_or(Error::InvalidArgs)?
        .unpack();

    let unlock_args = match UnlockCustodianViaRevertWitnessReader::verify(&data, false) {
        Ok(_) => UnlockCustodianViaRevertWitness::new_unchecked(data),
        Err(_) => return Err(Error::InvalidArgs),
    };

    // the reverted deposit cell must exists
    let deposit_cell_index =
        search_lock_hash(&unlock_args.deposit_lock_hash().unpack(), Source::Output)
            .ok_or(Error::InvalidOutput)?;
    let deposit_lock = load_cell_lock(deposit_cell_index, Source::Output)?;
    let deposit_lock_args = {
        let args: Bytes = deposit_lock.args().unpack();
        if args.len() < rollup_type_hash.len() {
            return Err(Error::InvalidArgs);
        }
        if args[..32] != rollup_type_hash {
            return Err(Error::InvalidArgs);
        }

        match DepositLockArgsReader::verify(&args.slice(32..), false) {
            Ok(_) => DepositLockArgs::new_unchecked(args.slice(32..)),
            Err(_) => return Err(Error::InvalidOutput),
        }
    };
    if deposit_lock.code_hash().as_slice() != config.deposit_script_type_hash().as_slice()
        || deposit_lock.hash_type() != ScriptHashType::Type.into()
        || deposit_lock_args.as_slice() != lock_args.deposit_lock_args().as_slice()
    {
        return Err(Error::InvalidOutput);
    }

    // check deposit block is reverted
    let deposit_block_hash = lock_args.deposit_block_hash();
    let mut rollup_action_witness = [0u8; MAX_ROLLUP_WITNESS_SIZE];
    let rollup_action = {
        let index = search_rollup_cell(&rollup_type_hash, Source::Output)
            .ok_or(Error::RollupCellNotFound)?;
        parse_rollup_action(&mut rollup_action_witness, index, Source::Output)?
    };

    match rollup_action.to_enum() {
        RollupActionUnionReader::RollupSubmitBlock(args) => {
            if args
                .reverted_block_hashes()
                .iter()
                .any(|hash| hash.as_slice() == deposit_block_hash.as_slice())
            {
                return Ok(());
            }
            Err(Error::InvalidRevertedBlocks)
        }
        _ => Err(Error::InvalidRevertedBlocks),
    }
}
