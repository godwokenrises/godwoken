// Import from `core` instead of from `std` since we are in no-std mode
use core::{cell::Cell, result::Result};

// Import heap related library from `alloc`
// https://doc.rust-lang.org/alloc/index.html
use alloc::{collections::BTreeMap, vec::Vec};
use validator_utils::{
    ckb_std::high_level::load_witness_args,
    search_cells::{search_lock_hash, search_lock_hashes},
};

// Import CKB syscalls and structures
// https://nervosnetwork.github.io/ckb-std/riscv64imac-unknown-none-elf/doc/ckb_std/index.html
use crate::{
    ckb_std::{
        ckb_constants::Source,
        ckb_types::{bytes::Bytes, core::ScriptHashType, prelude::*},
        high_level::{
            load_cell_capacity, load_cell_data, load_cell_lock, load_cell_type,
            load_cell_type_hash, QueryIter,
        },
    },
    types::{CellValue, CustodianCell, DepositionRequestCell, WithdrawalCell},
};

use crate::error::Error;
use crate::types::BlockContext;

use gw_common::{
    builtins::CKB_SUDT_ACCOUNT_ID, error::Error as StateError, state::State, CKB_SUDT_SCRIPT_ARGS,
    H256, ROLLUP_LOCK_CODE_HASH,
};
use gw_types::{
    packed::{
        Block, CustodianLockArgs, CustodianLockArgsReader, DepositionLockArgs,
        DepositionLockArgsReader, L2Block, RollupActionUnion, RollupActionUnionReader, Script,
        UnlockAccountWitness, UnlockAccountWitnessReader, WithdrawalLockArgs, WithdrawalLockArgsReader,
        WithdrawalRequest,
    },
    prelude::Unpack as GodwokenTypesUnpack,
};
fn fetch_sudt_script_hash(index: usize, source: Source) -> Result<Option<[u8; 32]>, Error> {
    let sudt_code_hash: [u8; 32] = unreachable!();
    match load_cell_type(index, source)? {
        Some(type_) => {
            if type_.hash_type() == ScriptHashType::Data.into()
                && type_.code_hash().unpack() == sudt_code_hash
            {
                return Ok(load_cell_type_hash(index, source)?);
            }
            Err(Error::SUDT)
        }
        None => Ok(None),
    }
}

fn fetch_capacity_and_sudt_value(index: usize, source: Source) -> Result<CellValue, Error> {
    let capacity = load_cell_capacity(index, source)?;
    let value = match fetch_sudt_script_hash(index, source)? {
        Some(sudt_script_hash) => {
            let data = load_cell_data(index, source)?;
            let mut buf = [0u8; 16];
            buf.copy_from_slice(&data[..16]);
            let amount = u128::from_le_bytes(buf);
            CellValue {
                sudt_script_hash: sudt_script_hash.into(),
                amount,
                capacity,
            }
        }
        None => CellValue {
            sudt_script_hash: H256::zero(),
            amount: 0,
            capacity,
        },
    };
    Ok(value)
}

// fn collect_deposition_requests(rollup_id: &[u8; 32]) -> Result<Vec<DepositionRequest>, Error> {
//     let input_cell_locks: Vec<_> = QueryIter::new(load_cell_lock, Source::Input).collect();
//     // ensure no rollup lock
//     if input_cell_locks
//         .iter()
//         .find(|lock| lock.code_hash().unpack() == ROLLUP_LOCK_CODE_HASH)
//         .is_some()
//     {
//         return Err(Error::UnexpectedRollupLock);
//     }
//     // find deposition requests
//     input_cell_locks
//         .into_iter()
//         .enumerate()
//         .filter_map(|(i, lock)| {
//             if !(lock.hash_type() == ScriptHashType::Data.into()
//                 && lock.code_hash().unpack() == DEPOSITION_CODE_HASH)
//             {
//                 return None;
//             }
//             let args: Bytes = lock.args().unpack();
//             let deposition_args = match DepositionLockArgsReader::verify(&args, false) {
//                 Ok(_) => DepositionLockArgs::new_unchecked(args),
//                 Err(_) => {
//                     return Some(Err(Error::Encoding));
//                 }
//             };

//             // ignore deposition request that do not belong to Rollup
//             if &deposition_args.rollup_type_hash().unpack() != rollup_id {
//                 return None;
//             }

//             // get token_id
//             let token_id = match fetch_sudt_script_hash(i, Source::Input) {
//                 Ok(token_id) => token_id,
//                 Err(err) => return Some(Err(err)),
//             };
//             let value = match fetch_sudt_value(i, Source::Input, &token_id) {
//                 Ok(value) => value,
//                 Err(err) => {
//                     return Some(Err(err));
//                 }
//             };
//             Some(Ok(DepositionRequest {
//                 token_id,
//                 value,
//                 pubkey_hash: deposition_args.pubkey_hash().unpack(),
//                 account_id: deposition_args.account_id().unpack(),
//             }))
//         })
//         .collect()
// }

pub fn build_l2_sudt_script(l1_sudt_script_hash: [u8; 32]) -> Script {
    unreachable!()
    // let args = Bytes::from(l1_sudt_script_hash.to_vec());
    // Script::new_builder()
    //     .args(args.pack())
    //     .code_hash({
    //         let code_hash: [u8; 32] = (*SUDT_VALIDATOR_CODE_HASH).into();
    //         code_hash.pack()
    //     })
    //     .hash_type(ScriptHashType::Data.into())
    //     .build()
}

fn collect_withdrawal_locks(
    rollup_type_hash: &[u8; 32],
    withdrawal_lock_code_hash: &[u8; 32],
    source: Source,
) -> Result<Vec<WithdrawalCell>, Error> {
    QueryIter::new(load_cell_lock, source)
        .enumerate()
        .filter_map(|(index, lock)| {
            let is_withdrawal_lock = &lock.args().as_slice()[..32] == rollup_type_hash
                && lock.as_slice() == withdrawal_lock_code_hash;
            if !is_withdrawal_lock {
                return None;
            }
            let raw_args = lock.args().as_slice()[32..].to_vec();
            let args = match WithdrawalLockArgsReader::verify(&raw_args, false) {
                Ok(_) => WithdrawalLockArgs::new_unchecked(raw_args.into()),
                Err(_) => {
                    return Some(Err(Error::Encoding));
                }
            };
            let value = match fetch_capacity_and_sudt_value(index, Source::Output) {
                Ok(value) => value,
                Err(err) => return Some(Err(err)),
            };
            Some(Ok(WithdrawalCell { index, args, value }))
        })
        .collect::<Result<_, Error>>()
}

fn collect_custodian_locks(
    rollup_type_hash: &[u8; 32],
    custodian_lock_code_hash: &[u8; 32],
    source: Source,
) -> Result<Vec<CustodianCell>, Error> {
    QueryIter::new(load_cell_lock, source)
        .enumerate()
        .filter_map(|(index, lock)| {
            let is_lock = &lock.args().as_slice()[..32] == rollup_type_hash
                && lock.as_slice() == custodian_lock_code_hash;
            if !is_lock {
                return None;
            }
            let raw_args = lock.args().as_slice()[32..].to_vec();
            let args = match CustodianLockArgsReader::verify(&raw_args, false) {
                Ok(_) => CustodianLockArgs::new_unchecked(raw_args.into()),
                Err(_) => {
                    return Some(Err(Error::Encoding));
                }
            };
            let value = match fetch_capacity_and_sudt_value(index, Source::Input) {
                Ok(value) => value,
                Err(err) => return Some(Err(err)),
            };
            let cell = CustodianCell { index, args, value };
            Some(Ok(cell))
        })
        .collect::<Result<_, Error>>()
}

fn collect_deposition_locks(
    rollup_type_hash: &[u8; 32],
    deposition_lock_code_hash: &[u8; 32],
    source: Source,
) -> Result<Vec<DepositionRequestCell>, Error> {
    QueryIter::new(load_cell_lock, source)
        .enumerate()
        .filter_map(|(index, lock)| {
            let is_lock = &lock.args().as_slice()[..32] == rollup_type_hash
                && lock.as_slice() == deposition_lock_code_hash;
            if !is_lock {
                return None;
            }
            let raw_args = lock.args().as_slice()[32..].to_vec();
            let args = match DepositionLockArgsReader::verify(&raw_args, false) {
                Ok(_) => DepositionLockArgs::new_unchecked(raw_args.into()),
                Err(_) => {
                    return Some(Err(Error::Encoding));
                }
            };
            let account_script_hash = args.layer2_lock().hash().into();
            let value = match fetch_capacity_and_sudt_value(index, Source::Input) {
                Ok(value) => value,
                Err(err) => return Some(Err(err)),
            };
            let cell = DepositionRequestCell {
                index,
                args,
                value,
                account_script_hash,
            };
            Some(Ok(cell))
        })
        .collect::<Result<_, Error>>()
}

fn check_withdrawal_cells(
    context: &mut BlockContext,
    mut withdrawal_requests: Vec<WithdrawalRequest>,
    withdrawal_cells: &[WithdrawalCell],
) -> Result<(), Error> {
    // iter outputs withdrawal cells, check each cell has a corresponded withdrawal request
    for cell in withdrawal_cells {
        // check withdrawal cell block info
        let withdrawal_block_hash: [u8; 32] = cell.args.withdrawal_block_hash().unpack();
        if withdrawal_block_hash != context.block_hash
            || cell.args.withdrawal_block_number().unpack() != context.number
        {
            return Err(Error::InvalidWithdrawal);
        }

        let cell_account_script_hash: H256 = cell.args.account_script_hash().unpack();
        // check that there is a corresponded withdrawal request
        let withdrawal_request = match withdrawal_requests.iter().position(|request| {
            let raw = request.raw();
            let account_script_hash: H256 = raw.account_script_hash().unpack();
            let sudt_script_hash: H256 = raw.sudt_script_hash().unpack();
            let amount: u128 = raw.amount().unpack();
            let capacity: u64 = raw.capacity().unpack();

            account_script_hash == cell_account_script_hash
                && sudt_script_hash == cell.value.sudt_script_hash
                && amount == cell.value.amount
                && capacity == cell.value.capacity
        }) {
            Some(index) => withdrawal_requests.remove(index),
            None => {
                return Err(Error::InvalidWithdrawal);
            }
        };
        // check that there is an input to unlock account
        let message = withdrawal_request.raw().hash().into();
        check_input_account_lock(cell_account_script_hash.into(), message)?;
    }
    // Some withdrawal requests hasn't has a corresponded withdrawal cell
    if !withdrawal_requests.is_empty() {
        return Err(Error::InvalidWithdrawal);
    }
    Ok(())
}

fn check_input_custodian_cells(
    context: &mut BlockContext,
    output_withdrawal_cells: Vec<WithdrawalCell>,
) -> Result<(), Error> {
    let custodian_lock_code_hash = unreachable!();
    let deposition_lock_code_hash = unreachable!();
    // collect input custodian cells
    let (finalized_custodian_cells, unfinalized_custodian_cells): (Vec<_>, Vec<_>) =
        collect_custodian_locks(
            &context.rollup_type_hash,
            custodian_lock_code_hash,
            Source::Input,
        )?
        .into_iter()
        .partition(|cell| {
            let number: u64 = cell.args.deposition_block_number().unpack();
            number <= context.finalized_number
        });
    // check finalized custodian cells == withdrawal cells
    // we only need to verify the assets is equal
    {
        let mut withdrawal_assets = BTreeMap::new();
        for withdrawal in output_withdrawal_cells {
            let sudt_balance = withdrawal_assets
                .entry(withdrawal.value.sudt_script_hash)
                .or_insert(0u128);
            *sudt_balance = sudt_balance
                .checked_add(withdrawal.value.amount)
                .ok_or(Error::AmountOverflow)?;
            let ckb_balance = withdrawal_assets
                .entry(CKB_SUDT_SCRIPT_ARGS.into())
                .or_insert(0u128);
            *ckb_balance = ckb_balance
                .checked_add(withdrawal.value.capacity.into())
                .ok_or(Error::AmountOverflow)?;
        }
        for cell in finalized_custodian_cells {
            let sudt_balance = withdrawal_assets
                .entry(cell.value.sudt_script_hash)
                .or_insert(0);
            *sudt_balance = sudt_balance
                .checked_sub(cell.value.amount)
                .ok_or(Error::AmountOverflow)?;
            let ckb_balance = withdrawal_assets
                .entry(CKB_SUDT_SCRIPT_ARGS.into())
                .or_insert(0);
            *ckb_balance = ckb_balance
                .checked_sub(cell.value.capacity.into())
                .ok_or(Error::AmountOverflow)?;
        }
        // failed to check the equality
        if !withdrawal_assets.values().all(|&v| v == 0) {
            return Err(Error::InvalidWithdrawal);
        }
    }

    // check unfinalized custodian cells == reverted deposition requests
    let mut reverted_deposition_cells = collect_deposition_locks(
        &context.rollup_type_hash,
        deposition_lock_code_hash,
        Source::Output,
    )?;
    for custodian_cell in unfinalized_custodian_cells {
        let index = reverted_deposition_cells
            .iter()
            .position(|cell| {
                custodian_cell.args.deposition_lock_args() == cell.args
                    && custodian_cell.value == cell.value
            })
            .ok_or(Error::InvalidWithdrawal)?;
        reverted_deposition_cells.remove(index);
    }
    if !reverted_deposition_cells.is_empty() {
        return Err(Error::InvalidWithdrawal);
    }
    Ok(())
}

fn check_output_custodian_cells(context: &mut BlockContext) -> Result<(), Error> {
    let custodian_lock_code_hash = unreachable!();
    let deposition_lock_code_hash = unreachable!();
    let withdrawal_lock_code_hash = unreachable!();
    // collect output custodian cells
    let (finalized_custodian_cells, unfinalized_custodian_cells): (Vec<_>, Vec<_>) =
        collect_custodian_locks(
            &context.rollup_type_hash,
            custodian_lock_code_hash,
            Source::Output,
        )?
        .into_iter()
        .partition(|cell| {
            let number: u64 = cell.args.deposition_block_number().unpack();
            number <= context.finalized_number
        });
    // check depositions request cells == unfinalized custodian cells
    let mut deposition_cells = collect_deposition_locks(
        &context.rollup_type_hash,
        deposition_lock_code_hash,
        Source::Output,
    )?;
    for custodian_cell in unfinalized_custodian_cells {
        let index = deposition_cells
            .iter()
            .position(|cell| {
                custodian_cell.args.deposition_lock_args() == cell.args
                    && custodian_cell.value == cell.value
            })
            .ok_or(Error::InvalidWithdrawal)?;
        deposition_cells.remove(index);
    }
    if !deposition_cells.is_empty() {
        return Err(Error::InvalidWithdrawal);
    }
    // check reverted withdrawals == finalized custodian cells
    {
        let reverted_withdrawals = collect_withdrawal_locks(
            &context.rollup_type_hash,
            withdrawal_lock_code_hash,
            Source::Input,
        )?;
        let mut reverted_withdrawal_assets = BTreeMap::new();
        for cell in reverted_withdrawals {
            let sudt_balance = reverted_withdrawal_assets
                .entry(cell.value.sudt_script_hash)
                .or_insert(0u128);
            *sudt_balance = sudt_balance
                .checked_add(cell.value.amount)
                .ok_or(Error::AmountOverflow)?;
            let ckb_balance = reverted_withdrawal_assets
                .entry(CKB_SUDT_SCRIPT_ARGS.into())
                .or_insert(0u128);
            *ckb_balance = ckb_balance
                .checked_add(cell.value.capacity.into())
                .ok_or(Error::AmountOverflow)?;
        }
        for cell in finalized_custodian_cells {
            let sudt_balance = reverted_withdrawal_assets
                .entry(cell.value.sudt_script_hash)
                .or_insert(0);
            *sudt_balance = sudt_balance
                .checked_sub(cell.value.amount)
                .ok_or(Error::AmountOverflow)?;
            let ckb_balance = reverted_withdrawal_assets
                .entry(CKB_SUDT_SCRIPT_ARGS.into())
                .or_insert(0);
            *ckb_balance = ckb_balance
                .checked_sub(cell.value.capacity.into())
                .ok_or(Error::AmountOverflow)?;
        }
        // check the equality
        if !reverted_withdrawal_assets.values().all(|&v| v == 0) {
            return Err(Error::InvalidWithdrawal);
        }
    }
    Ok(())
}

fn check_input_account_lock(account_script_hash: H256, message: H256) -> Result<(), Error> {
    // check inputs has accout lock cell
    for index in search_lock_hashes(&account_script_hash.into(), Source::Input) {
        // parse witness lock
        let witness_args = load_witness_args(index, Source::Input)?;
        let lock: Bytes = witness_args
            .lock()
            .to_opt()
            .ok_or(Error::InvalidWithdrawal)?
            .unpack();
        let unlock_account_witness = match UnlockAccountWitnessReader::verify(&lock, false) {
            Ok(_) => UnlockAccountWitness::new_unchecked(lock),
            Err(_) => return Err(Error::InvalidWithdrawal),
        };
        // check message
        let actual_message: H256 = unlock_account_witness.message().unpack();
        if actual_message == message {
            return Ok(());
        }
    }
    Err(Error::InvalidWithdrawal)
}

fn mint_layer2_sudt(context: &mut BlockContext) -> Result<(), Error> {
    let deposition_lock_code_hash = unreachable!();
    let deposition_requests = collect_deposition_locks(
        &context.rollup_type_hash,
        deposition_lock_code_hash,
        Source::Input,
    )?;
    for request in &deposition_requests {
        // find or create user account
        let id = match context.get_account_id_by_script_hash(&request.account_script_hash.into())? {
            Some(id) => id,
            None => context.create_account(request.account_script_hash)?,
        };
        // mint CKB
        context.mint_sudt(CKB_SUDT_ACCOUNT_ID, id, request.value.capacity.into())?;
        if request.value.sudt_script_hash.as_slice() == &CKB_SUDT_SCRIPT_ARGS {
            if request.value.amount != 0 {
                // SUDT amount must equals to zero if sudt script hash is equals to CKB_SUDT_SCRIPT_ARGS
                return Err(Error::SUDT);
            }
            continue;
        }
        // find or create Simple UDT account
        let l2_sudt_script = build_l2_sudt_script(request.value.sudt_script_hash.into());
        let l2_sudt_script_hash: [u8; 32] = l2_sudt_script.hash();
        let sudt_id = match context.get_account_id_by_script_hash(&l2_sudt_script_hash.into())? {
            Some(id) => id,
            None => context.create_account(l2_sudt_script_hash.into())?,
        };
        // prevent fake CKB SUDT, the caller should filter these invalid depositions
        if sudt_id == CKB_SUDT_ACCOUNT_ID {
            return Err(Error::SUDT);
        }
        // mint SUDT
        context.mint_sudt(sudt_id, id, request.value.amount)?;
    }

    Ok(())
}

fn burn_layer2_sudt(
    context: &mut BlockContext,
    withdrawal_cells: &[WithdrawalCell],
) -> Result<(), Error> {
    for request in withdrawal_cells {
        let l2_sudt_script_hash: [u8; 32] =
            build_l2_sudt_script(request.value.sudt_script_hash.into()).hash();
        // find user account
        let id = context
            .get_account_id_by_script_hash(&request.args.account_script_hash().unpack())?
            .ok_or(StateError::MissingKey)?;
        // burn CKB
        context.burn_sudt(CKB_SUDT_ACCOUNT_ID, id, request.value.capacity.into())?;
        // find Simple UDT account
        let sudt_id = context
            .get_account_id_by_script_hash(&l2_sudt_script_hash.into())?
            .ok_or(StateError::MissingKey)?;
        // burn sudt
        context.burn_sudt(sudt_id, id, request.value.amount)?;
    }

    Ok(())
}

/// Verify Deposition & Withdrawal
pub fn verify(context: &mut BlockContext, block: &L2Block) -> Result<(), Error> {
    // Mint token: deposition requests -> layer2 SUDT
    mint_layer2_sudt(context)?;
    // build withdrawl_cells
    let withdrawal_lock_code_hash = unreachable!();
    let withdrawal_cells = collect_withdrawal_locks(
        &context.rollup_type_hash,
        withdrawal_lock_code_hash,
        Source::Output,
    )?;
    // Withdrawal token: Layer2 SUDT -> withdrawals
    burn_layer2_sudt(context, &withdrawal_cells)?;
    // Check new cells and reverted cells
    let withdrawal_requests = block.withdrawal_requests().into_iter().collect();
    check_withdrawal_cells(context, withdrawal_requests, &withdrawal_cells)?;
    check_input_custodian_cells(context, withdrawal_cells)?;
    check_output_custodian_cells(context)?;
    Ok(())
}
