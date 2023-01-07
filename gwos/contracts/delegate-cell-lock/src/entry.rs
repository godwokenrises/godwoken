use ckb_std::{
    ckb_constants::Source,
    high_level::{
        load_cell_data, load_cell_lock_hash, load_cell_type_hash, load_script, load_witness_args,
        QueryIter,
    },
};

use crate::error::Error;

/// A simple lock that delegates verification to another lock via a
/// delegate-cell. This is supposed to be used on the rollup cell to support
/// changing block producer key by e.g. multisig. In a production deployment,
/// the delegate cell is supposed to be a type-id cell with a multisig lock, and
/// delegates to secp256k1/blake160 lock with the current block producer key.
///
/// Lock args: type hash (and usually type-id) of delegate-cell.
///
/// Delegate-cell data should be blake160 hash of a lock script.
///
/// Unlock is successful if there is an input with a matching lock script.
pub fn main() -> Result<(), Error> {
    let script = load_script()?;
    let args = script.as_reader().args().raw_data();

    let witness_arg = load_witness_args(0, Source::GroupInput)?;
    if witness_arg.as_reader().lock().is_some() {
        return Err(Error::InvalidWitnessArgs);
    }

    // Get expected type hash from args.
    let expected_type_hash: [u8; 32] = args.try_into().map_err(|_| Error::InvalidArgs)?;

    // Load cell dep data by type hash.
    let cell_dep_idx = QueryIter::new(load_cell_type_hash, Source::CellDep)
        .position(|type_hash| type_hash == Some(expected_type_hash))
        .ok_or(Error::CellDepNotFound)?;
    let data = load_cell_data(cell_dep_idx, Source::CellDep)?;
    let expected_lock_hash_160: [u8; 20] =
        data.try_into().map_err(|_| Error::CellDepDataInvalid)?;

    // Check that there is a matching input.
    let found = QueryIter::new(load_cell_lock_hash, Source::Input)
        .any(|lock_hash| lock_hash[..20] == expected_lock_hash_160);
    if found {
        Ok(())
    } else {
        Err(Error::InputNotFound)
    }
}
