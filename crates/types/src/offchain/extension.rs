use crate::packed::{AccountMerkleState, Byte32, TransactionKey, TxReceipt};
use crate::prelude::*;
use sparse_merkle_tree::H256;

use super::RunResult;

impl TransactionKey {
    pub fn build_transaction_key(block_hash: Byte32, index: u32) -> Self {
        let mut key = [0u8; 36];
        key[..32].copy_from_slice(block_hash.as_slice());
        // use BE, so we have a sorted bytes representation
        key[32..].copy_from_slice(&index.to_be_bytes());
        key.pack()
    }
}

impl TxReceipt {
    pub fn build_receipt(
        tx_witness_hash: H256,
        run_result: RunResult,
        post_state: AccountMerkleState,
    ) -> Self {
        TxReceipt::new_builder()
            .tx_witness_hash(tx_witness_hash.pack())
            .post_state(post_state)
            .read_data_hashes(
                run_result
                    .read_data
                    .into_iter()
                    .map(|(hash, _)| hash.pack())
                    .collect::<Vec<_>>()
                    .pack(),
            )
            .logs(run_result.logs.pack())
            .build()
    }
}
