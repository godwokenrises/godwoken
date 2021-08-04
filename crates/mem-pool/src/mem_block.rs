use std::collections::HashMap;

use gw_common::{merkle_utils::calculate_state_checkpoint, H256};
use gw_types::{
    offchain::DepositInfo,
    packed::{BlockInfo, L2Block, TxReceipt},
    prelude::*,
};

#[derive(Debug, Default)]
pub struct MemBlock {
    block_producer_id: u32,
    /// Tx receipts
    tx_receipts: HashMap<H256, TxReceipt>,
    /// Finalized txs
    txs: Vec<H256>,
    /// Finalized withdrawals
    withdrawals: Vec<H256>,
    /// Finalized withdrawals
    deposits: Vec<DepositInfo>,
    /// State check points
    state_checkpoints: Vec<H256>,
    /// The state before txs
    txs_prev_state_checkpoint: Option<H256>,
    /// Mem block info
    block_info: BlockInfo,
}

impl MemBlock {
    pub fn block_info(&self) -> &BlockInfo {
        &self.block_info
    }

    pub fn reset(&mut self, tip: &L2Block) {
        unreachable!();
        let estimate_timestamp = unreachable!();
        let tip_number: u64 = tip.raw().number().unpack();
        let number = tip_number + 1;
        self.block_info = BlockInfo::new_builder()
            .block_producer_id(self.block_producer_id.pack())
            .timestamp(estimate_timestamp)
            .number(number.pack())
            .build();
    }

    pub fn push_withdrawal(&mut self, withdrawal_hash: H256, state_checkpoint: H256) {
        assert!(self.txs.is_empty());
        assert!(self.deposits.is_empty());
        self.withdrawals.push(withdrawal_hash);
        self.state_checkpoints.push(state_checkpoint);
    }

    pub fn push_deposits(&mut self, deposit_cells: Vec<DepositInfo>, prev_state_checkpoint: H256) {
        assert!(self.txs_prev_state_checkpoint.is_none());
        self.deposits = deposit_cells;
        self.txs_prev_state_checkpoint = Some(prev_state_checkpoint);
    }

    pub fn push_tx(&mut self, tx_hash: H256, receipt: TxReceipt) {
        let post_state = receipt.post_state();
        let state_checkpoint = calculate_state_checkpoint(
            &post_state.merkle_root().unpack(),
            post_state.count().unpack(),
        );
        self.txs.push(tx_hash);
        self.tx_receipts.insert(tx_hash, receipt);
        self.state_checkpoints.push(state_checkpoint);
    }

    pub fn withdrawals(&self) -> &[H256] {
        &self.withdrawals
    }

    pub fn deposits(&self) -> &[DepositInfo] {
        &self.deposits
    }

    pub fn txs(&self) -> &[H256] {
        &self.txs
    }

    pub fn tx_receipts(&self) -> &HashMap<H256, TxReceipt> {
        &self.tx_receipts
    }

    pub fn state_checkpoints(&self) -> &[H256] {
        &self.state_checkpoints
    }

    pub fn block_producer_id(&self) -> u32 {
        self.block_producer_id
    }
}
