use ckb_std::{
    ckb_constants::Source,
    high_level::{
        load_cell_capacity, load_cell_data, load_cell_lock_hash, load_cell_type_hash, QueryIter,
    },
    syscalls::SysError,
};
use alloc::vec::Vec;
use gw_types::{
    packed::{GlobalState, GlobalStateReader},
    prelude::*,
};

pub fn search_rollup_cell(rollup_type_hash: &[u8; 32]) -> Option<usize> {
    QueryIter::new(load_cell_type_hash, Source::Input)
        .position(|type_hash| type_hash.as_ref() == Some(rollup_type_hash))
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

pub fn fetch_token_amount(
    owner_lock_hash: &[u8; 32],
    token_type: &TokenType,
    source: Source,
) -> Result<u128, SysError> {
    let amount = QueryIter::new(load_cell_lock_hash, source)
        .into_iter()
        .enumerate()
        .filter_map(|(i, lock_hash)| {
            if &lock_hash != owner_lock_hash {
                return None;
            }
            match load_cell_type_hash(i, source) {
                Ok(Some(type_hash)) if &TokenType::SUDT(type_hash) == token_type => {
                    Some(load_cell_data(i, source).map(|data| {
                        let mut buf = [0u8; 16];
                        buf.copy_from_slice(&data[..16]);
                        u128::from_le_bytes(buf)
                    }))
                }
                Ok(None) if &TokenType::CKB == token_type => {
                    Some(load_cell_capacity(i, source).map(|capacity| capacity as u128))
                }
                Err(err) => {
                    return Some(Err(err));
                }
                Ok(_) => None,
            }
        })
        .collect::<Result<Vec<u128>, SysError>>()?
        .into_iter()
        .sum::<u128>();
    Ok(amount)
}
