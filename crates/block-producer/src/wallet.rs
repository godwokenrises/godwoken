use anyhow::Result;
use gw_types::packed::Transaction;

use crate::transaction_skeleton::TransactionSkeleton;

pub struct Wallet;

impl Wallet {
    pub fn lock_hash(&self) -> [u8; 32] {
        unimplemented!()
    }

    // sign message
    pub fn sign_message(&self, msg: [u8; 32]) -> [u8; 65] {
        unimplemented!()
    }

    pub fn sign_tx_skeleton(&self, tx_skeleton: TransactionSkeleton) -> Result<Transaction> {
        let signature_entries = tx_skeleton.signature_entries();
        let mut signatures = Vec::new();
        // TODO
        // for message in signature_entries {
        //     signatures.push(wallet.sign(message));
        // }
        let tx = tx_skeleton.seal(signature_entries, signatures)?;
        unimplemented!()
    }
}
