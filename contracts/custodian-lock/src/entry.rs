// Import from `core` instead of from `std` since we are in no-std mode
use core::result::Result;

use gw_common::DEPOSITION_LOCK_CODE_HASH;
use validator_utils::{
    ckb_std::high_level::load_cell_lock,
    search_cells::{
        parse_rollup_action, search_lock_hash, search_rollup_cell, search_rollup_state,
    },
};

// Import CKB syscalls and structures
// https://nervosnetwork.github.io/ckb-std/riscv64imac-unknown-none-elf/doc/ckb_std/index.html
use crate::ckb_std::{
    ckb_constants::Source, ckb_types::bytes::Bytes, ckb_types::prelude::Unpack as CKBUnpack,
    high_level::load_script, high_level::load_witness_args,
};
use gw_types::{
    packed::{
        CustodianLockArgs, CustodianLockArgsReader, RollupActionUnion,
        UnlockCustodianViaRevertWitness, UnlockCustodianViaRevertWitnessReader,
    },
    prelude::*,
};

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

    let deposition_block_number: u64 = lock_args.deposition_block_number().unpack();
    let last_finalized_block_number: u64 = global_state.last_finalized_block_number().unpack();

    if deposition_block_number <= last_finalized_block_number {
        // this custodian lock is already finalized, rollup will handle the logic
        return Ok(());
    }

    // otherwise, the submitter try to prove the deposit is reverted.

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

    // the reverted deposition cell must exists
    let deposition_cell_index =
        search_lock_hash(&unlock_args.deposition_lock_hash().unpack(), Source::Output)
            .ok_or(Error::InvalidOutput)?;
    let deposition_lock = load_cell_lock(deposition_cell_index, Source::Output)?;
    let deposition_lock_code_hash = deposition_lock.code_hash().unpack();
    if deposition_lock_code_hash != DEPOSITION_LOCK_CODE_HASH
        || deposition_lock.args().as_slice() != lock_args.deposition_lock_args().as_slice()
    {
        return Err(Error::InvalidOutput);
    }

    // check deposition block is reverted
    let deposition_block_hash = lock_args.deposition_block_hash();
    let rollup_action = {
        let index = search_rollup_cell(&rollup_type_hash, Source::Output)
            .ok_or(Error::RollupCellNotFound)?;
        parse_rollup_action(index, Source::Output)?
    };

    match rollup_action.to_enum() {
        RollupActionUnion::RollupSubmitBlock(args) => {
            if args
                .reverted_block_hashes()
                .into_iter()
                .find(|hash| hash == &deposition_block_hash)
                .is_some()
            {
                return Ok(());
            }
            Err(Error::InvalidRevertedBlocks)
        }
        _ => Err(Error::InvalidRevertedBlocks),
    }
}
