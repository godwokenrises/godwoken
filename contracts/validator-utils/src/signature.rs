use crate::{error::Error, search_cells::search_lock_hashes};
use ckb_std::{
    ckb_constants::Source, ckb_types::prelude::Unpack as CKBUnpack, high_level::load_witness_args,
};
use gw_common::H256;
use gw_types::{
    bytes::Bytes,
    packed::{UnlockAccountWitness, UnlockAccountWitnessReader},
    prelude::*,
};

pub fn check_input_account_lock(account_script_hash: H256, message: H256) -> Result<(), Error> {
    // check inputs has accout lock cell
    for index in search_lock_hashes(&account_script_hash.into(), Source::Input) {
        // parse witness lock
        let witness_args = load_witness_args(index, Source::Input)?;
        let lock: Bytes = witness_args
            .lock()
            .to_opt()
            .ok_or(Error::InvalidAccountLockCell)?
            .unpack();
        let unlock_account_witness = match UnlockAccountWitnessReader::verify(&lock, false) {
            Ok(_) => UnlockAccountWitness::new_unchecked(lock),
            Err(_) => return Err(Error::InvalidAccountLockCell),
        };
        // check message
        let actual_message: H256 = unlock_account_witness.message().unpack();
        if actual_message == message {
            return Ok(());
        }
    }
    Err(Error::InvalidAccountLockCell)
}
