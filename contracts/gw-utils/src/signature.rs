use core::convert::TryFrom;

use crate::{cells::utils::search_lock_hashes, error::Error};
use ckb_std::{ckb_constants::Source, debug, syscalls::load_cell_data};
use gw_common::H256;
use gw_types::core::SigningType;

/// Check l2 account signature cell
pub fn check_l2_account_signature_cell(
    script_hash: &H256,
    expected_signing_type: SigningType,
    message: H256,
) -> Result<(), Error> {
    debug!("Check l2 account signature for message {:?}", message);
    // search layer2 account lock cell from inputs
    for index in search_lock_hashes(&(*script_hash).into(), Source::Input) {
        // expected data is equals to onetime_lock_hash(32 bytes) | sign type (1 byte) | message(32 bytes)
        let mut data = [0u8; 33];
        let len = load_cell_data(&mut data, 32, index, Source::Input)?;

        // skip if the data isn't 32 length
        if len != data.len() {
            continue;
        }

        let signing_type = match SigningType::try_from(data[0]) {
            Ok(type_) => type_,
            Err(_err) => continue,
        };

        if signing_type != expected_signing_type {
            continue;
        }

        if &data[1..] == message.as_slice() {
            return Ok(());
        }
    }
    Err(Error::AccountLockCellNotFound)
}
