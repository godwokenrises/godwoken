// Import from `core` instead of from `std` since we are in no-std mode
use core::result::Result;

// Import heap related library from `alloc`
// https://doc.rust-lang.org/alloc/index.html
use alloc::vec;
use gw_common::{
    h256_ext::H256Ext,
    smt::{Blake2bHasher, CompiledMerkleProof},
    CUSTODIAN_LOCK_CODE_HASH, H256,
};
use gw_types::packed::{
    CustodianLockArgs, CustodianLockArgsReader, UnlockWithdrawalUnion, WithdrawalLockArgs,
    WithdrawalLockArgsReader,
};
use validator_utils::{
    ckb_std::high_level::load_cell_lock,
    search_cells::{fetch_token_amount, search_lock_hash, search_rollup_state, TokenType},
};

// Import CKB syscalls and structures
// https://nervosnetwork.github.io/ckb-std/riscv64imac-unknown-none-elf/doc/ckb_std/index.html
use crate::ckb_std::{
    ckb_constants::Source,
    ckb_types::{self, bytes::Bytes, prelude::Unpack as CKBUnpack},
    high_level::{
        load_cell_capacity, load_cell_data_hash, load_cell_type_hash, load_script,
        load_witness_args,
    },
};

use crate::error::Error;
use gw_types::{
    packed::{UnlockWithdrawal, UnlockWithdrawalReader},
    prelude::*,
};

const FINALIZED_BLOCK_NUMBER: u64 = 0;
const FINALIZED_BLOCK_HASH: [u8; 32] = [0u8; 32];

/// args: rollup_type_hash | withdrawal lock args
fn parse_lock_args(
    script: &ckb_types::packed::Script,
) -> Result<([u8; 32], WithdrawalLockArgs), Error> {
    let mut rollup_type_hash = [0u8; 32];
    let args: Bytes = script.args().unpack();
    if args.len() < rollup_type_hash.len() {
        return Err(Error::InvalidArgs);
    }
    rollup_type_hash.copy_from_slice(&args[..32]);
    match WithdrawalLockArgsReader::verify(&args.slice(32..), false) {
        Ok(()) => Ok((
            rollup_type_hash,
            WithdrawalLockArgs::new_unchecked(args.slice(32..)),
        )),
        Err(_) => Err(Error::InvalidArgs),
    }
}

pub fn main() -> Result<(), Error> {
    let script = load_script()?;
    let (rollup_type_hash, lock_args) = parse_lock_args(&script)?;

    // load unlock arguments from witness
    let witness_args = load_witness_args(0, Source::GroupInput)?;
    let unlock_args = {
        let unlock_args: Bytes = witness_args
            .lock()
            .to_opt()
            .ok_or(Error::ProofNotFound)?
            .unpack();
        match UnlockWithdrawalReader::verify(&unlock_args, false) {
            Ok(()) => UnlockWithdrawal::new_unchecked(unlock_args),
            Err(_) => return Err(Error::ProofNotFound),
        }
    };

    // read global state from rollup cell
    match search_rollup_state(&rollup_type_hash, Source::CellDep)? {
        Some(global_state) => {
            // read merkle proof
            let reverted_block_root: [u8; 32] = global_state.reverted_block_root().unpack();
            let block_hash: H256 = lock_args.withdrawal_block_hash().unpack();

            match unlock_args.to_enum() {
                UnlockWithdrawalUnion::UnlockWithdrawalViaRevert(unlock_args) => {
                    // we can revert withdrawal at anytime even it is 'finalized'
                    let merkle_proof = CompiledMerkleProof(unlock_args.block_proof().unpack());

                    // merkle proof the block is reverted
                    if !merkle_proof
                        .verify::<Blake2bHasher>(
                            &reverted_block_root.into(),
                            vec![(block_hash, H256::one())],
                        )
                        .map_err(|_| Error::MerkleProof)?
                    {
                        return Err(Error::MerkleProof);
                    }
                    let custodian_lock_hash: [u8; 32] = unlock_args.custodian_lock_hash().unpack();
                    // check there are a reverted custodian lock in the output
                    let custodian_cell_index =
                        match search_lock_hash(&custodian_lock_hash, Source::Output) {
                            Some(index) => index,
                            None => return Err(Error::InvalidOutput),
                        };

                    // check reverted custodian deposition info.
                    let custodian_lock = load_cell_lock(custodian_cell_index, Source::Output)?;
                    let custodian_lock_args = {
                        let args: Bytes = custodian_lock.args().unpack();
                        match CustodianLockArgsReader::verify(&args, false) {
                            Ok(_) => CustodianLockArgs::new_unchecked(args),
                            Err(_) => return Err(Error::InvalidOutput),
                        }
                    };
                    let custodian_code_hash: [u8; 32] = custodian_lock.code_hash().unpack();
                    let custodian_deposition_block_hash: [u8; 32] =
                        custodian_lock_args.deposition_block_hash().unpack();
                    let custodian_deposition_block_number: u64 =
                        custodian_lock_args.deposition_block_number().unpack();
                    if custodian_code_hash != CUSTODIAN_LOCK_CODE_HASH
                        || custodian_deposition_block_hash != FINALIZED_BLOCK_HASH
                        || custodian_deposition_block_number != FINALIZED_BLOCK_NUMBER
                    {
                        return Err(Error::InvalidOutput);
                    }

                    // check capacity, data_hash, type_hash
                    check_output_cell_has_same_content(custodian_cell_index)?;
                    Ok(())
                }
                UnlockWithdrawalUnion::UnlockWithdrawalViaFinalize(_unlock_args) => {
                    // let merkle_proof = CompiledMerkleProof(unlock_args.block_proof().unpack());
                    // check finality
                    let withdrawal_block_number: u64 = lock_args.withdrawal_block_number().unpack();
                    let last_finalized_block_number: u64 =
                        global_state.last_finalized_block_number().unpack();

                    if withdrawal_block_number > last_finalized_block_number {
                        // not yet finalized
                        return Err(Error::InvalidArgs);
                    }

                    // prove the block is not reverted
                    // TODO: add this part back when revert flow is properly implemented.
                    // if !merkle_proof
                    //     .verify::<Blake2bHasher>(
                    //         &reverted_block_root.into(),
                    //         vec![(block_hash.into(), H256::zero())],
                    //     )
                    //     .map_err(|_| Error::MerkleProof)?
                    // {
                    //     return Err(Error::MerkleProof);
                    // }

                    // withdrawal lock is finalized, unlock for owner
                    if search_lock_hash(&lock_args.owner_lock_hash().unpack(), Source::Input)
                        .is_none()
                    {
                        return Err(Error::OwnerCellNotFound);
                    }
                    Ok(())
                }
                _ => {
                    // unknown unlock condition
                    Err(Error::InvalidArgs)
                }
            }
        }
        None => {
            // rollup cell does not in this tx, which means this is a buying tx
            // return success if tx has enough output send to owner

            let unlock_args = match unlock_args.to_enum() {
                UnlockWithdrawalUnion::UnlockWithdrawalViaTrade(unlock_args) => unlock_args,
                _ => return Err(Error::InvalidArgs),
            };
            // make sure output >= input + sell_amount
            let payment_lock_hash = lock_args.payment_lock_hash().unpack();
            let sudt_script_hash: [u8; 32] = lock_args.sudt_script_hash().unpack();
            let token_type: TokenType = sudt_script_hash.into();
            let input_token = fetch_token_amount(&payment_lock_hash, &token_type, Source::Input)?;
            let output_token = fetch_token_amount(&payment_lock_hash, &token_type, Source::Output)?;
            let sell_amount: u128 = lock_args.sell_amount().unpack();
            let sell_capacity: u64 = lock_args.sell_capacity().unpack();
            let expected_output_amount = input_token
                .total_token_amount
                .checked_add(sell_amount)
                .ok_or(Error::OverflowAmount)?;
            let expected_output_capacity = input_token
                .total_capacity
                .checked_add(sell_capacity as u128)
                .ok_or(Error::OverflowAmount)?;
            if output_token.total_token_amount < expected_output_amount
                || output_token.total_capacity < expected_output_capacity
            {
                return Err(Error::InsufficientAmount);
            }

            // make sure the output should only change owner_lock_hash field
            let new_lock_hash = unlock_args.owner_lock().hash();

            let index = match search_lock_hash(&new_lock_hash, Source::Output) {
                Some(i) => i,
                None => return Err(Error::InvalidOutput),
            };
            // check new withdraw cell
            check_output_cell_has_same_content(index)?;
            Ok(())
        }
    }
}

fn check_output_cell_has_same_content(output_index: usize) -> Result<(), Error> {
    if load_cell_capacity(0, Source::GroupInput)?
        != load_cell_capacity(output_index, Source::Output)?
    {
        return Err(Error::InvalidOutput);
    }

    if load_cell_data_hash(0, Source::GroupInput)?
        != load_cell_data_hash(output_index, Source::Output)?
    {
        return Err(Error::InvalidOutput);
    }

    if load_cell_type_hash(0, Source::GroupInput)?
        != load_cell_type_hash(output_index, Source::Output)?
    {
        return Err(Error::InvalidOutput);
    }
    Ok(())
}
