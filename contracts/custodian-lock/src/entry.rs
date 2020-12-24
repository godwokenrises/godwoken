// Import from `core` instead of from `std` since we are in no-std mode
use core::result::Result;

use alloc::vec;
use gw_common::{
    h256_ext::H256Ext,
    smt::{Blake2bHasher, CompiledMerkleProof},
    DEPOSITION_LOCK_CODE_HASH, H256,
};
use validator_utils::{
    ckb_std::high_level::load_cell_lock,
    search_cells::{search_lock_hash, search_rollup_state},
};

// Import CKB syscalls and structures
// https://nervosnetwork.github.io/ckb-std/riscv64imac-unknown-none-elf/doc/ckb_std/index.html
use crate::ckb_std::{
    ckb_constants::Source, ckb_types::bytes::Bytes, ckb_types::prelude::Unpack as CKBUnpack,
    high_level::load_script, high_level::load_witness_args,
};
use gw_types::{
    packed::{
        CustodianLockArgs, CustodianLockArgsReader, UnlockCustodianViaRevert,
        UnlockCustodianViaRevertReader,
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
    match CustodianLockArgsReader::verify(&args, false) {
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

    // otherwise, the submitter try to proof the deposition is reverted.

    // read the proof
    let witness_args = load_witness_args(0, Source::GroupInput)?;
    let data: Bytes = witness_args
        .lock()
        .to_opt()
        .ok_or(Error::ProofNotFound)?
        .unpack();

    let unlock_args = match UnlockCustodianViaRevertReader::verify(&data, false) {
        Ok(_) => UnlockCustodianViaRevert::new_unchecked(data),
        Err(_) => return Err(Error::ProofNotFound),
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

    // check reverted_blocks merkle proof
    let reverted_block_root: [u8; 32] = global_state.reverted_block_root().unpack();
    let block_hash = lock_args.deposition_block_hash().unpack();

    let merkle_proof = CompiledMerkleProof(unlock_args.block_proof().unpack());
    if merkle_proof
        .verify::<Blake2bHasher>(
            &reverted_block_root.into(),
            vec![(block_hash.into(), H256::one())],
        )
        .map_err(|_err| Error::MerkleProof)?
    {
        Ok(())
    } else {
        Err(Error::MerkleProof)
    }
}
