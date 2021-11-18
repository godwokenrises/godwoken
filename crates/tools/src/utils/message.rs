use ckb_fixed_hash::H256;
use gw_types::{packed::RawL2Transaction, prelude::*};

use crate::hasher::{CkbHasher, EthHasher};

pub fn generate_transaction_message_to_sign(
    raw_l2transaction: &RawL2Transaction,
    rollup_type_hash: &H256,
    sender_script_hash: &H256,
    receiver_script_hash: &H256,
) -> H256 {
    let raw_data = raw_l2transaction.as_slice();
    let rollup_type_hash_data = rollup_type_hash.as_bytes();

    let digest = CkbHasher::new()
        .update(rollup_type_hash_data)
        .update(sender_script_hash.as_bytes())
        .update(receiver_script_hash.as_bytes())
        .update(raw_data)
        .finalize();

    let message = EthHasher::new()
        .update("\x19Ethereum Signed Message:\n32")
        .update(digest.as_bytes())
        .finalize();

    message
}
