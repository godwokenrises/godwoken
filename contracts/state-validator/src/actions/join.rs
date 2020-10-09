// Import from `core` instead of from `std` since we are in no-std mode
use core::result::Result;

// Import heap related library from `alloc`
// https://doc.rust-lang.org/alloc/index.html
use alloc::{collections::BTreeMap, vec::Vec};

// Import CKB syscalls and structures
// https://nervosnetwork.github.io/ckb-std/riscv64imac-unknown-none-elf/doc/ckb_std/index.html
use ckb_std::{
    ckb_constants::Source,
    ckb_types::{bytes::Bytes, core::ScriptHashType, prelude::*},
    high_level::{
        load_cell_capacity, load_cell_data, load_cell_lock, load_cell_type, load_cell_type_hash,
        QueryIter,
    },
};

use crate::context::Context;
use crate::error::Error;

use gw_types::{
    packed::{DepositionLockArgs, DepositionLockArgsReader, L2Block},
    prelude::Unpack as GodwokenTypesUnpack,
};

// code hashes
// TODO fill real code hash
const DEPOSITION_CODE_HASH: [u8; 32] = [0u8; 32];
const SUDT_CODE_HASH: [u8; 32] = [0u8; 32];
const ROLLUP_LOCK_CODE_HASH: [u8; 32] = [0u8; 32];

const CKB_TOKEN_ID: [u8; 32] = [0u8; 32];

struct DepositionRequest {
    pubkey_hash: [u8; 20],
    account_id: u32,
    token_id: [u8; 32],
    value: u128,
}

fn fetch_token_id(index: usize, source: Source) -> Result<[u8; 32], Error> {
    match load_cell_type(index, source)? {
        Some(type_) => {
            if type_.hash_type() == ScriptHashType::Data.into()
                && type_.code_hash().unpack() == SUDT_CODE_HASH
            {
                return load_cell_type_hash(index, source)?.ok_or(Error::SUDT);
            }
            Err(Error::SUDT)
        }
        None => Ok(CKB_TOKEN_ID),
    }
}

fn fetch_sudt_value(index: usize, source: Source, token_id: &[u8; 32]) -> Result<u128, Error> {
    if token_id == &CKB_TOKEN_ID {
        return Ok(load_cell_capacity(index, source).map(Into::into)?);
    }
    let data = load_cell_data(index, source)?;
    let mut buf = [0u8; 16];
    buf.copy_from_slice(&data[..16]);
    Ok(u128::from_le_bytes(buf))
}

fn collect_deposition_requests(rollup_id: &[u8; 32]) -> Result<Vec<DepositionRequest>, Error> {
    let input_cell_locks: Vec<_> = QueryIter::new(load_cell_lock, Source::Input).collect();
    // ensure no rollup lock
    if input_cell_locks
        .iter()
        .find(|lock| lock.code_hash().unpack() == ROLLUP_LOCK_CODE_HASH)
        .is_some()
    {
        return Err(Error::UnexpectedRollupLock);
    }
    // find deposition requests
    input_cell_locks
        .into_iter()
        .enumerate()
        .filter_map(|(i, lock)| {
            if !(lock.hash_type() == ScriptHashType::Data.into()
                && lock.code_hash().unpack() == DEPOSITION_CODE_HASH)
            {
                return None;
            }
            let args: Bytes = lock.args().unpack();
            let deposition_args = match DepositionLockArgsReader::verify(&args, false) {
                Ok(_) => DepositionLockArgs::new_unchecked(args),
                Err(_) => {
                    return Some(Err(Error::Encoding));
                }
            };

            // ignore deposition request that do not belong to Rollup
            if &deposition_args.rollup_type_id().unpack() != rollup_id {
                return None;
            }

            // get token_id
            let token_id = match fetch_token_id(i, Source::Input) {
                Ok(token_id) => token_id,
                Err(err) => return Some(Err(err)),
            };
            let value = match fetch_sudt_value(i, Source::Input, &token_id) {
                Ok(value) => value,
                Err(err) => {
                    return Some(Err(err));
                }
            };
            Some(Ok(DepositionRequest {
                token_id,
                value,
                pubkey_hash: deposition_args.pubkey_hash().unpack(),
                account_id: deposition_args.account_id().unpack(),
            }))
        })
        .collect()
}

// check outputs coins match the deposited coins
fn check_outputs_rollup_lock(deposition_requests: &[DepositionRequest]) -> Result<(), Error> {
    // group deposition value by token id
    let mut deposition_by_token_id: BTreeMap<[u8; 32], u128> = BTreeMap::default();
    for request in deposition_requests {
        deposition_by_token_id
            .entry(request.token_id)
            .and_modify(|amount| *amount += request.value)
            .or_insert(request.value);
    }

    // minus output cell value form deposition values
    for i in QueryIter::new(load_cell_lock, Source::Output)
        .into_iter()
        .enumerate()
        .filter_map(|(i, lock)| {
            if !(lock.code_hash().unpack() == ROLLUP_LOCK_CODE_HASH
                && lock.hash_type() == ScriptHashType::Data.into())
            {
                return None;
            }
            Some(i)
        })
    {
        let token_id = fetch_token_id(i, Source::Output)?;
        let value = fetch_sudt_value(i, Source::Output, &token_id)?;
        let total_value = deposition_by_token_id
            .get_mut(&token_id)
            .ok_or(Error::DepositionValue)?;
        *total_value = total_value
            .checked_sub(value)
            .ok_or(Error::DepositionValue)?;
        if *total_value == 0 {
            deposition_by_token_id.remove(&token_id);
        }
    }

    // the output value should equals to deposited value
    if !deposition_by_token_id.is_empty() {
        return Err(Error::DepositionValue);
    }
    Ok(())
}

/// Handle join
pub fn handle(context: &mut Context, _block: &L2Block) -> Result<(), Error> {
    // 1. find all inputs which use the deposition-lock
    // 2. find or create accounts accoding to requests
    // 3. deposit balance to account (how? call contract, or directly alter the balance)

    let deposition_requests = collect_deposition_requests(&context.rollup_type_id)?;
    check_outputs_rollup_lock(&deposition_requests)?;

    // mint token
    for request in deposition_requests {
        if request.account_id == 0 {
            let id = context.create_account(request.pubkey_hash)?;
            context.mint_sudt(&request.token_id, id, request.value)?;
        } else {
            context.mint_sudt(&request.token_id, request.account_id, request.value)?;
        }
    }

    Ok(())
}
