use crate::{error::Error, search_cells::search_lock_hashes};
use ckb_std::{ckb_constants::Source, syscalls::load_cell_data};
use gw_common::H256;

pub fn check_input_account_lock(
    account_script_hash: H256,
    expected_message: H256,
) -> Result<(), Error> {
    // check inputs has accout lock cell
    for index in search_lock_hashes(&account_script_hash.into(), Source::Input) {
        // load message from input
        let mut message = [0u8; 32];
        let loaded_len = load_cell_data(&mut message, 0, index, Source::Input)?;
        if message.len() != loaded_len {
            // the message is corrupted, the signature verification script will return failure
            return Err(Error::InvalidAccountLockCell);
        }
        if H256::from(message) == expected_message {
            return Ok(());
        }
    }
    Err(Error::InvalidAccountLockCell)
}
