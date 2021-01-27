use alloc::vec::Vec;
use ckb_std::{
    ckb_constants::Source,
    high_level::{
        load_cell_capacity, load_cell_data, load_cell_data_hash, load_cell_lock_hash,
        load_cell_type_hash, load_witness_args, QueryIter,
    },
    syscalls::SysError,
};
use gw_types::{
    bytes::Bytes,
    packed::{GlobalState, GlobalStateReader, RollupAction, RollupActionReader},
    prelude::*,
};

use crate::error::Error;

pub fn search_rollup_cell(rollup_type_hash: &[u8; 32], source: Source) -> Option<usize> {
    QueryIter::new(load_cell_type_hash, source)
        .position(|type_hash| type_hash.as_ref() == Some(rollup_type_hash))
}

pub fn search_rollup_config_cell(rollup_config_hash: &[u8; 32]) -> Option<usize> {
    QueryIter::new(load_cell_data_hash, Source::CellDep)
        .position(|data_hash| data_hash.as_ref() == rollup_config_hash)
}

pub fn search_rollup_state(
    rollup_type_hash: &[u8; 32],
    source: Source,
) -> Result<Option<GlobalState>, SysError> {
    let index = match QueryIter::new(load_cell_type_hash, source)
        .position(|type_hash| type_hash.as_ref() == Some(rollup_type_hash))
    {
        Some(i) => i,
        None => return Ok(None),
    };
    let data = load_cell_data(index, source)?;
    match GlobalStateReader::verify(&data, false) {
        Ok(()) => Ok(Some(GlobalState::new_unchecked(data.into()))),
        Err(_) => Err(SysError::Encoding),
    }
}

pub fn search_lock_hashes(owner_lock_hash: &[u8; 32], source: Source) -> Vec<usize> {
    QueryIter::new(load_cell_lock_hash, source)
        .enumerate()
        .filter_map(|(i, lock_hash)| {
            if &lock_hash == owner_lock_hash {
                Some(i)
            } else {
                None
            }
        })
        .collect()
}

pub fn search_lock_hash(owner_lock_hash: &[u8; 32], source: Source) -> Option<usize> {
    QueryIter::new(load_cell_lock_hash, source).position(|lock_hash| &lock_hash == owner_lock_hash)
}

#[derive(PartialEq, Eq, Debug, Clone)]
pub enum TokenType {
    CKB,
    SUDT([u8; 32]),
}

impl From<[u8; 32]> for TokenType {
    fn from(sudt_script_hash: [u8; 32]) -> Self {
        if sudt_script_hash == [0u8; 32] {
            TokenType::CKB
        } else {
            TokenType::SUDT(sudt_script_hash)
        }
    }
}

#[derive(PartialEq, Eq, Debug, Clone)]
pub struct CellTokenAmount {
    pub total_token_amount: u128,
    pub total_capacity: u128,
}

pub fn fetch_token_amount(
    owner_lock_hash: &[u8; 32],
    token_type: &TokenType,
    source: Source,
) -> Result<CellTokenAmount, Error> {
    let mut total_token_amount = 0u128;
    let mut total_capacity = 0u128;
    for (i, lock_hash) in QueryIter::new(load_cell_lock_hash, source)
        .into_iter()
        .enumerate()
    {
        if &lock_hash != owner_lock_hash {
            continue;
        }

        let capacity = load_cell_capacity(i, source)?;
        total_capacity = total_capacity
            .checked_add(capacity as u128)
            .ok_or(Error::OverflowAmount)?;
        let amount = match load_cell_type_hash(i, source)? {
            Some(type_hash) if &TokenType::SUDT(type_hash) == token_type => {
                let data = load_cell_data(i, source)?;
                let mut buf = [0u8; 16];
                buf.copy_from_slice(&data[..16]);
                u128::from_le_bytes(buf)
            }
            _ => 0,
        };
        total_token_amount = total_token_amount
            .checked_add(amount)
            .ok_or(Error::OverflowAmount)?;
    }
    Ok(CellTokenAmount {
        total_token_amount,
        total_capacity,
    })
}

pub fn parse_rollup_action(index: usize, source: Source) -> Result<RollupAction, Error> {
    use ckb_std::ckb_types::prelude::Unpack;

    let witness_args = load_witness_args(index, source)?;
    let output_type: Bytes = witness_args
        .output_type()
        .to_opt()
        .ok_or_else(|| Error::Encoding)?
        .unpack();
    match RollupActionReader::verify(&output_type, false) {
        Ok(_) => Ok(RollupAction::new_unchecked(output_type)),
        Err(_) => Err(Error::Encoding),
    }
}
