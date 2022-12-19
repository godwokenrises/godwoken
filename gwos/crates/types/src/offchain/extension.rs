use core::convert::TryInto;

use crate::h256::H256;
use crate::packed::{
    AccountMerkleState, Byte32, CompactMemBlock, DeprecatedCompactMemBlock, GlobalState,
    GlobalStateV0, MemBlock, RawWithdrawalRequest, TransactionKey, TxReceipt, WithdrawalKey,
    WithdrawalRequestExtra,
};
use crate::prelude::*;
use ckb_types::error::VerificationError;

use super::RunResult;

impl TransactionKey {
    pub fn build_transaction_key(block_hash: Byte32, index: u32) -> Self {
        let mut key = [0u8; 36];
        key[..32].copy_from_slice(block_hash.as_slice());
        // use BE, so we have a sorted bytes representation
        key[32..].copy_from_slice(&index.to_be_bytes());
        key.pack()
    }

    pub fn block_hash(&self) -> H256 {
        self.as_slice()[..32].try_into().unwrap()
    }

    pub fn index(&self) -> u32 {
        u32::from_be_bytes(self.as_slice()[32..].try_into().unwrap())
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
            .exit_code((run_result.exit_code as u8).into())
            .tx_witness_hash(tx_witness_hash.pack())
            .post_state(post_state)
            .read_data_hashes(
                run_result
                    .read_data_hashes
                    .into_iter()
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
            Err(_) => match DeprecatedCompactMemBlock::from_slice(slice) {
                Ok(deprecated) => {
                    let block = CompactMemBlock::new_builder()
                        .txs(deprecated.txs())
                        .withdrawals(deprecated.withdrawals())
                        .deposits(deprecated.deposits())
                        .build();
                    Ok(block)
                }
                Err(_) => MemBlock::from_slice(slice).map(Into::into),
            },
        }
    }
}

impl WithdrawalRequestExtra {
    pub fn hash(&self) -> [u8; 32] {
        self.request().hash()
    }

    pub fn witness_hash(&self) -> [u8; 32] {
        self.request().witness_hash()
    }

    pub fn raw(&self) -> RawWithdrawalRequest {
        self.request().raw()
    }
}

impl PartialEq for WithdrawalRequestExtra {
    fn eq(&self, other: &Self) -> bool {
        self.as_slice() == other.as_slice()
    }
}

impl Eq for WithdrawalRequestExtra {}
