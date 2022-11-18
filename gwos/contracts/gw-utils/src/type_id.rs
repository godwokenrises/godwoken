use ckb_std::{
    ckb_constants::Source,
    debug,
    high_level::{load_cell_type_hash, load_script_hash, QueryIter},
    syscalls::{load_cell, load_input, SysError},
};
use gw_common::blake2b::new_blake2b;

use crate::error::Error;

pub const TYPE_ID_SIZE: usize = 32;

/// check type id
/// type_id: the first 32-bytes of the current script.args
/// notice the type_id must be included in the script.args
pub fn check_type_id(type_id: [u8; 32]) -> Result<(), Error> {
    // check there is only one type id cell in each input/output group
    let has_second_input_type_id_cell = has_type_id_cell(1, Source::GroupInput);
    let has_second_output_type_id_cell = has_type_id_cell(1, Source::GroupOutput);
    if has_second_input_type_id_cell || has_second_output_type_id_cell {
        debug!("There are only be at most one input and at most one output type id cell");
        return Err(Error::InvalidTypeID);
    }
    let has_first_input_type_id_cell = has_type_id_cell(0, Source::GroupInput);
    // we already has type_id, just return OK
    if has_first_input_type_id_cell {
        return Ok(());
    }
    // no type_id cell in the input, we are on the creation of a new type_id cell
    // search current output index.
    // (since we have no input in the group, we must have at least one output)
    let script_hash = load_script_hash()?;
    let output_index: u64 = QueryIter::new(load_cell_type_hash, Source::Output)
        .position(|type_hash| type_hash == Some(script_hash))
        .ok_or(Error::InvalidTypeID)? as u64;
    // The type ID is calculated as the blake2b (with CKB's personalization) of
    // the first CellInput in current transaction, and the created output cell
    // index(in 64-bit little endian unsigned integer).
    let mut buf = [0u8; 128];
    let input_len = load_input(&mut buf, 0, 0, Source::Input)?;
    let mut hasher = new_blake2b();
    hasher.update(&buf[..input_len]);
    hasher.update(&output_index.to_le_bytes());
    let mut expected_type_id = [0u8; 32];
    hasher.finalize(&mut expected_type_id);
    if type_id != expected_type_id {
        debug!(
            "type_id: {:?}, expected_type_id: {:?}",
            type_id, expected_type_id
        );
        return Err(Error::InvalidTypeID);
    }
    Ok(())
}

fn has_type_id_cell(index: usize, source: Source) -> bool {
    let mut buf = [0u8; 0];
    match load_cell(&mut buf, 0, index, source) {
        Ok(_) => true,
        Err(SysError::LengthNotEnough(..)) => true,
        _ => false,
    }
}
