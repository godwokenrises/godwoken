// Import from `core` instead of from `std` since we are in no-std mode
use core::result::Result;

// Import heap related library from `alloc`
// https://doc.rust-lang.org/alloc/index.html
use alloc::{vec, vec::Vec};

// Import CKB syscalls and structures
// https://nervosnetwork.github.io/ckb-std/riscv64imac-unknown-none-elf/doc/ckb_std/index.html
use ckb_std::{
    ckb_types::{bytes::Bytes, prelude::*},
    debug,
    high_level::{load_script, load_tx_hash, load_witness_args},
};

use crate::error::Error;

// code hashes
const DEPOSITION_CODE_HASH: [u8; 32] = unreachable!();
const SUDT_CODE_HASH: [u8; 32] = unreachable!();

const CKB_TOKEN_ID: [u8; 32] = [0u8; 32];

struct DepositionRequest{
    pubkey_hash: [u8; 20],
    account_id: u32,
    token_id: [u8; 32],
    value: u128,
}

fn fetch_token_id(index: usize, source: Source) -> Result<[u8; 32], Error> {
    match load_cell_type(index, source)? {
        Some(type_) => {
            if type_._hash_type = HashType::Data && type_.code_hash == SUDT_CODE_HASH {
                return load_cell_type_hash(index, source)?.ok_or(Error::InvalidSUDT)?;
            }
            Err(Error::InvalidSUDT)
        }
        None => CKB_TOKEN_ID,
    }
}

fn fetch_sudt_value(index: usize, source: Source, token_id: &[u8; 32]) -> u128 {
    if token_id == CKB_TOKEN_ID {
        return load_cell_capacity(index, source)?.into();
    }
    let data = load_cell_data(index, source)?;
    let mut buf = [0u8; 16];
    buf.copy_from_slice(&data[..16]);
    u128::from_le_bytes(buf)
}

fn collect_deposition_requests() -> Result<Vec<DepositionRequest>, Error> {
    QueryIter::new(load_cell_lock, Source::Input)
        .into_iter()
        .enumerate()
        .filter(|(i, lock)| {
            lock._hash_type = HashType::Data && lock.code_hash == DEPOSITION_CODE_HASH
        })
        .map(|(i, lock)| {
            let args: Bytes = lock.args().unpack();
            let deposition_args = match DepositionArgsReader::verify(&args, false) {
                Ok(_) => (i, DepositionArgs::new_unchecked(args)),
                Err(_) => {
                    return Err(Error::Encoding);
                }
            };
            // get token_id
            let token_id = fetch_token_id(i, Source::Input)?;
            let value = fetch_sudt_value(i, Source::Input, &token_id);
            Ok(DepositionRequest {
                token_id,
                value,
                pubkey_hash: deposition_args.pubkey_hash().unpack(),
                account_id: deposition_args.account_id().unpack(),
            })
        })
        .collect()
}

fn handle_join(context: &mut Context, raw_block: &RawBlock) -> Result<(), Error> {
    // 1. find all inputs which use the deposition-lock
    // 2. find or create accounts accoding to requests
    // 3. deposit balance to account (how? call contract, or directly alter the balance)
    let deposition_requests = collect_deposition_request()?;
    unimplemented!()
}
