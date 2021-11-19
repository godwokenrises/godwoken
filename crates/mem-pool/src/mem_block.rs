use std::{collections::HashSet, time::Duration};

use gw_common::{merkle_utils::calculate_state_checkpoint, H256};
use gw_types::{
    offchain::{CollectedCustodianCells, DepositInfo},
    packed::{self, AccountMerkleState, BlockInfo, L2Block, TxReceipt},
    prelude::*,
};

pub struct MemBlockContent {
    pub withdrawals: Vec<H256>,
    pub txs: Vec<H256>,
}

#[derive(Debug, Default, Clone)]
pub struct MemBlock {
    block_producer_id: u32,
    /// Finalized txs
    txs: Vec<H256>,
    /// Txs set
    txs_set: HashSet<H256>,
    /// Finalized withdrawals
    withdrawals: Vec<H256>,
    /// Finalized custodians to produce finalized withdrawals
    finalized_custodians: Option<CollectedCustodianCells>,
    /// Withdrawals set
    withdrawals_set: HashSet<H256>,
    /// Finalized withdrawals
    deposits: Vec<DepositInfo>,
    /// State check points
    state_checkpoints: Vec<H256>,
    /// The state before txs
    txs_prev_state_checkpoint: Option<H256>,
    /// Mem block info
    block_info: BlockInfo,
    /// Mem block prev merkle state
    prev_merkle_state: AccountMerkleState,
    /// Mem block post merkle state
    post_merkle_state: AccountMerkleState,
    /// touched keys
    touched_keys: HashSet<H256>,
}

impl MemBlock {
    pub fn new(block_info: BlockInfo, prev_merkle_state: AccountMerkleState) -> Self {
        MemBlock {
            block_producer_id: block_info.block_producer_id().unpack(),
            block_info,
            prev_merkle_state,
            ..Default::default()
        }
    }

    /// Initialize MemBlock with block producer
    pub fn with_block_producer(block_producer_id: u32) -> Self {
        MemBlock {
            block_producer_id,
            ..Default::default()
        }
    }

    pub fn block_info(&self) -> &BlockInfo {
        &self.block_info
    }

    pub fn reset(&mut self, tip: &L2Block, estimated_timestamp: Duration) -> MemBlockContent {
        log::debug!("[mem-block] reset");
        // update block info
        let tip_number: u64 = tip.raw().number().unpack();
        let number = tip_number + 1;
        self.block_info = BlockInfo::new_builder()
            .block_producer_id(self.block_producer_id.pack())
            .timestamp((estimated_timestamp.as_millis() as u64).pack())
            .number(number.pack())
            .build();
        self.prev_merkle_state = tip.raw().post_account();
        // mem block content
        let content = MemBlockContent {
            txs: self.txs.clone(),
            withdrawals: self.withdrawals.clone(),
        };
        // reset status
        self.clear();
        content
    }

    pub fn clear(&mut self) {
        self.txs.clear();
        self.txs_set.clear();
        self.withdrawals.clear();
        self.withdrawals_set.clear();
        self.finalized_custodians = None;
        self.deposits.clear();
        self.state_checkpoints.clear();
        self.txs_prev_state_checkpoint = None;
        self.touched_keys.clear();
    }

    pub fn push_withdrawal(&mut self, withdrawal_hash: H256, state_checkpoint: H256) {
        assert!(self.txs.is_empty());
        assert!(self.deposits.is_empty());
        self.withdrawals.push(withdrawal_hash);
        self.withdrawals_set.insert(withdrawal_hash);
        self.state_checkpoints.push(state_checkpoint);
    }

    pub fn set_finalized_custodians(&mut self, finalized_custodians: CollectedCustodianCells) {
        assert!(self.finalized_custodians.is_none());
        self.finalized_custodians = Some(finalized_custodians);
    }

    pub fn push_deposits(&mut self, deposit_cells: Vec<DepositInfo>, prev_state_checkpoint: H256) {
        assert!(self.txs_prev_state_checkpoint.is_none());
        self.deposits = deposit_cells;
        self.txs_prev_state_checkpoint = Some(prev_state_checkpoint);
    }

    pub fn push_tx(&mut self, tx_hash: H256, receipt: &TxReceipt) {
        let post_state = receipt.post_state();
        let state_checkpoint = calculate_state_checkpoint(
            &post_state.merkle_root().unpack(),
            post_state.count().unpack(),
        );
        log::debug!(
            "[mem-block] push tx {} state {}",
            hex::encode(tx_hash.as_slice()),
            hex::encode(state_checkpoint.as_slice())
        );
        self.txs.push(tx_hash);
        self.txs_set.insert(tx_hash);
        self.state_checkpoints.push(state_checkpoint);
    }

    pub fn append_touched_keys<I: Iterator<Item = H256>>(&mut self, keys: I) {
        self.touched_keys.extend(keys)
    }

    pub fn withdrawals(&self) -> &[H256] {
        &self.withdrawals
    }

    pub fn finalized_custodians(&self) -> Option<&CollectedCustodianCells> {
        self.finalized_custodians.as_ref()
    }

    pub fn withdrawals_set(&self) -> &HashSet<H256> {
        &self.withdrawals_set
    }

    pub fn deposits(&self) -> &[DepositInfo] {
        &self.deposits
    }

    pub fn txs(&self) -> &[H256] {
        &self.txs
    }

    pub fn txs_set(&self) -> &HashSet<H256> {
        &self.txs_set
    }

    pub fn state_checkpoints(&self) -> &[H256] {
        &self.state_checkpoints
    }

    pub fn block_producer_id(&self) -> u32 {
        self.block_producer_id
    }

    pub fn touched_keys(&self) -> &HashSet<H256> {
        &self.touched_keys
    }

    pub fn txs_prev_state_checkpoint(&self) -> Option<H256> {
        self.txs_prev_state_checkpoint
    }

    pub fn prev_merkle_state(&self) -> &AccountMerkleState {
        &self.prev_merkle_state
    }

    pub fn set_post_merkle_state(&mut self, state: AccountMerkleState) {
        self.post_merkle_state = state;
    }

    pub fn post_merkle_state(&self) -> &AccountMerkleState {
        &self.post_merkle_state
    }

    pub fn pack(&self) -> packed::MemBlock {
        let touched_keys = self.touched_keys().iter().cloned().collect::<Vec<_>>();

        packed::MemBlock::new_builder()
            .block_producer_id(self.block_producer_id.pack())
            .txs(self.txs.pack())
            .withdrawals(self.withdrawals.pack())
            .deposits(self.deposits.pack())
            .state_checkpoints(self.state_checkpoints.pack())
            .txs_prev_state_checkpoint(self.txs_prev_state_checkpoint.pack())
            .block_info(self.block_info.clone())
            .prev_merkle_state(self.prev_merkle_state.clone())
            .post_merkle_state(self.post_merkle_state.clone())
            .touched_keys(touched_keys.pack())
            .build()
    }

    pub fn unpack(mem_block: packed::MemBlock) -> Self {
        let txs: Vec<_> = mem_block.txs().unpack();
        let txs_set = txs.iter().cloned().collect();
        let withdrawals: Vec<_> = mem_block.withdrawals().unpack();
        let withdrawals_set = withdrawals.iter().cloned().collect();
        let touched_keys = mem_block.touched_keys().into_iter().map(|h| h.unpack());

        MemBlock {
            block_producer_id: mem_block.block_producer_id().unpack(),
            txs,
            txs_set,
            withdrawals,
            withdrawals_set,
            finalized_custodians: None,
            deposits: mem_block.deposits().unpack(),
            state_checkpoints: mem_block.state_checkpoints().unpack(),
            txs_prev_state_checkpoint: mem_block.txs_prev_state_checkpoint().unpack(),
            block_info: mem_block.block_info(),
            prev_merkle_state: mem_block.prev_merkle_state(),
            post_merkle_state: mem_block.post_merkle_state(),
            touched_keys: touched_keys.collect(),
        }
    }
}
