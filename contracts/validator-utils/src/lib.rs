#![no_std]

// re-export ckb-std
pub use ckb_std;
use ckb_std::{
    ckb_constants::Source,
    high_level::{load_cell_data, load_cell_lock_hash, load_cell_type_hash, QueryIter},
    syscalls::SysError,
};
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

pub fn search_owner_cell(owner_lock_hash: &[u8; 32]) -> Option<usize> {
    QueryIter::new(load_cell_lock_hash, Source::Input)
        .position(|lock_hash| &lock_hash == owner_lock_hash)
}
