use gw_hash::blake2b::new_blake2b;
use sparse_merkle_tree::H256;

use crate::packed::{RawL2Transaction, RawWithdrawalRequest};
use crate::prelude::*;

impl RawL2Transaction {
    pub fn calc_message(
        &self,
        rollup_type_script_hash: &H256,
        sender_script_hash: &H256,
        receiver_script_hash: &H256,
    ) -> H256 {
        let mut hasher = new_blake2b();
        hasher.update(rollup_type_script_hash.as_slice());
        hasher.update(sender_script_hash.as_slice());
        hasher.update(receiver_script_hash.as_slice());
        hasher.update(self.as_slice());
        let mut message = [0u8; 32];
        hasher.finalize(&mut message);
        message.into()
    }
}

impl RawWithdrawalRequest {
    pub fn calc_message(&self, rollup_type_script_hash: &H256) -> H256 {
        let mut hasher = new_blake2b();
        hasher.update(rollup_type_script_hash.as_slice());
        hasher.update(self.as_slice());
        let mut message = [0u8; 32];
        hasher.finalize(&mut message);
        message.into()
    }
}
