use crate::packed::{
    AccountMerkleState, Byte32, CompactMemBlock, GlobalState, GlobalStateV0, MemBlock,
    TransactionKey, TxReceipt, WithdrawalKey, WithdrawalRequestExtra,
};
use crate::prelude::*;
use ckb_types::error::VerificationError;
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

impl WithdrawalKey {
    pub fn build_withdrawal_key(block_hash: Byte32, index: u32) -> Self {
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

pub fn global_state_from_slice(slice: &[u8]) -> Result<GlobalState, VerificationError> {
    match GlobalState::from_slice(slice) {
        Ok(state) => Ok(state),
        Err(_) => GlobalStateV0::from_slice(slice).map(Into::into),
    }
}

impl From<MemBlock> for CompactMemBlock {
    fn from(block: MemBlock) -> Self {
        CompactMemBlock::new_builder()
            .txs(block.txs())
            .withdrawals(block.withdrawals())
            .deposits(block.deposits())
            .build()
    }
}

impl CompactMemBlock {
    pub fn from_full_compatible_slice(slice: &[u8]) -> Result<CompactMemBlock, VerificationError> {
        match CompactMemBlock::from_slice(slice) {
            Ok(block) => Ok(block),
            Err(_) => MemBlock::from_slice(slice).map(Into::into),
        }
    }
}

impl std::hash::Hash for WithdrawalRequestExtra {
    #[inline]
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.as_slice().hash(state)
    }
}

impl PartialEq for WithdrawalRequestExtra {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.as_slice() == other.as_slice()
    }
}

impl Eq for WithdrawalRequestExtra {}
