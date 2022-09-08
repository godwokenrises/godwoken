use std::{collections::HashSet, time::Duration};

use gw_common::{
    merkle_utils::calculate_state_checkpoint, registry_address::RegistryAddress, H256,
};
use gw_types::{
    bytes::Bytes,
    offchain::DepositInfo,
    packed::{self, AccountMerkleState, BlockInfo, L2Block},
    prelude::*,
};

pub struct MemBlockContent {
    pub withdrawals: Vec<H256>,
    pub txs: Vec<H256>,
}

#[derive(Debug, Default, Clone)]
pub struct MemBlock {
    block_producer: RegistryAddress,
    /// Finalized txs
    txs: Vec<H256>,
    /// Txs set
    txs_set: HashSet<H256>,
    /// Finalized withdrawals
    withdrawals: Vec<H256>,
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
    /// Touched keys
    touched_keys: HashSet<H256>,
    /// Post merkle states
    withdrawal_post_states: Vec<AccountMerkleState>,
    deposit_post_states: Vec<AccountMerkleState>,
    tx_post_states: Vec<AccountMerkleState>,
    /// Touched keys vector
    withdrawal_touched_keys_vec: Vec<Vec<H256>>,
    deposit_touched_keys_vec: Vec<Vec<H256>>,
}

impl MemBlock {
    pub(crate) fn new(block_info: BlockInfo, prev_merkle_state: AccountMerkleState) -> Self {
        let block_producer: Bytes = block_info.block_producer().unpack();
        let block_producer =
            RegistryAddress::from_slice(&block_producer).expect("invalid block producer registry");
        MemBlock {
            block_producer,
            block_info,
            prev_merkle_state,
            ..Default::default()
        }
    }

    /// Initialize MemBlock with block producer
    pub(crate) fn with_block_producer(block_producer: RegistryAddress) -> Self {
        MemBlock {
            block_producer,
            ..Default::default()
        }
    }

    pub fn block_info(&self) -> &BlockInfo {
        &self.block_info
    }

    pub(crate) fn reset(
        &mut self,
        tip: &L2Block,
        estimated_timestamp: Duration,
    ) -> MemBlockContent {
        log::debug!("[mem-block] reset");
        // update block info
        let tip_number: u64 = tip.raw().number().unpack();
        let number = tip_number + 1;
        self.block_info = BlockInfo::new_builder()
            .block_producer(self.block_producer.to_bytes().pack())
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

    pub(crate) fn clear(&mut self) {
        self.txs.clear();
        self.txs_set.clear();
        self.withdrawals.clear();
        self.withdrawals_set.clear();
        self.deposits.clear();
        self.state_checkpoints.clear();
        self.txs_prev_state_checkpoint = None;
        self.touched_keys.clear();
        self.withdrawal_post_states.clear();
        self.deposit_post_states.clear();
        self.tx_post_states.clear();
        self.withdrawal_touched_keys_vec.clear();
        self.deposit_touched_keys_vec.clear();
    }

    pub(crate) fn push_withdrawal<I: IntoIterator<Item = H256>>(
        &mut self,
        withdrawal_hash: H256,
        post_state: AccountMerkleState,
        touched_keys: I,
    ) {
        assert!(self.txs.is_empty());
        assert!(self.deposits.is_empty());

        let touched_keys: Vec<_> = touched_keys.into_iter().collect();
        self.withdrawals.push(withdrawal_hash);
        self.withdrawals_set.insert(withdrawal_hash);
        self.withdrawal_post_states.push(post_state.clone());
        self.withdrawal_touched_keys_vec.push(touched_keys.clone());

        let checkpoint = calculate_state_checkpoint(
            &post_state.merkle_root().unpack(),
            post_state.count().unpack(),
        );
        self.state_checkpoints.push(checkpoint);
        self.append_touched_keys(touched_keys);
    }

    pub(crate) fn force_reinject_withdrawal_hashes(&mut self, withdrawal_hashes: &[H256]) {
        assert!(self.withdrawals.is_empty());
        assert!(self.state_checkpoints.is_empty());
        assert!(self.deposits.is_empty());
        assert!(self.txs.is_empty());

        for withdrawal_hash in withdrawal_hashes {
            if !self.withdrawals_set.contains(withdrawal_hash) {
                self.withdrawals_set.insert(*withdrawal_hash);
                self.withdrawals.push(*withdrawal_hash);
            }
        }
    }

    pub(crate) fn push_deposits(
        &mut self,
        deposit_cells: Vec<DepositInfo>,
        post_states: Vec<AccountMerkleState>,
        touched_keys_vec: Vec<Vec<H256>>,
        txs_prev_state_checkpoint: H256,
    ) {
        assert!(self.txs_prev_state_checkpoint.is_none());
        assert_eq!(deposit_cells.len(), post_states.len());
        assert_eq!(deposit_cells.len(), touched_keys_vec.len());

        if let Some(txs_prev_state) = post_states.last().as_ref() {
            let checkpoint = calculate_state_checkpoint(
                &txs_prev_state.merkle_root().unpack(),
                txs_prev_state.count().unpack(),
            );
            assert_eq!(checkpoint, txs_prev_state_checkpoint);
        }

        self.deposits = deposit_cells;
        self.deposit_post_states = post_states;
        self.deposit_touched_keys_vec = touched_keys_vec.clone();
        self.txs_prev_state_checkpoint = Some(txs_prev_state_checkpoint);
        self.append_touched_keys(touched_keys_vec.into_iter().flatten());
    }

    pub(crate) fn push_tx(&mut self, tx_hash: H256, post_state: AccountMerkleState) {
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
        self.tx_post_states.push(post_state);

        self.state_checkpoints.push(state_checkpoint);
    }

    pub(crate) fn force_reinject_tx_hashes(&mut self, tx_hashes: &[H256]) {
        for tx_hash in tx_hashes {
            if !self.txs_set.contains(tx_hash) {
                self.txs_set.insert(*tx_hash);
                self.txs.push(*tx_hash);
            }
        }
    }

    pub(crate) fn clear_txs(&mut self) {
        self.txs_set.clear();
        self.txs.clear();
        self.touched_keys.clear();
        self.state_checkpoints.clear();
        self.txs_prev_state_checkpoint = None;
        self.tx_post_states.clear();
    }

    pub(crate) fn append_touched_keys<I: IntoIterator<Item = H256>>(&mut self, keys: I) {
        self.touched_keys.extend(keys)
    }

    pub fn withdrawals(&self) -> &[H256] {
        &self.withdrawals
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

    pub fn block_producer(&self) -> &RegistryAddress {
        &self.block_producer
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

    pub fn withdrawal_post_states(&self) -> &[AccountMerkleState] {
        &self.withdrawal_post_states
    }

    pub fn deposit_post_states(&self) -> &[AccountMerkleState] {
        &self.deposit_post_states
    }

    pub fn tx_post_states(&self) -> &[AccountMerkleState] {
        &self.tx_post_states
    }

    pub fn withdrawal_touched_keys_vec(&self) -> &[Vec<H256>] {
        &self.withdrawal_touched_keys_vec
    }

    pub fn deposit_touched_keys_vec(&self) -> &[Vec<H256>] {
        &self.deposit_touched_keys_vec
    }

    pub fn repackage(
        &self,
        withdrawals_count: usize,
        deposits_count: usize,
        txs_count: usize,
    ) -> (MemBlock, AccountMerkleState) {
        assert_eq!(
            self.withdrawal_post_states().len(),
            self.withdrawals().len()
        );
        assert_eq!(
            self.withdrawal_touched_keys_vec().len(),
            self.withdrawals().len()
        );
        assert_eq!(self.deposit_post_states().len(), self.deposits().len());
        assert_eq!(self.deposit_touched_keys_vec().len(), self.deposits().len());
        assert_eq!(self.tx_post_states().len(), self.txs().len());

        if withdrawals_count == self.withdrawals().len()
            && deposits_count == self.deposits().len()
            && txs_count == self.txs().len()
        {
            let post_state = {
                let state = { vec![self.prev_merkle_state()].into_iter() }
                    .chain(self.withdrawal_post_states())
                    .chain(self.deposit_post_states())
                    .chain(self.tx_post_states());
                // We have at least one state, mem_block.prev_merkle_state
                state.last().unwrap().clone()
            };

            return (self.clone(), post_state);
        }

        // Make sure we drop tx first, then deposits.
        if deposits_count != self.deposits().len() {
            assert_eq!(txs_count, 0);
        }
        if withdrawals_count != self.withdrawals().len() {
            assert_eq!(txs_count, 0);
            assert_eq!(deposits_count, 0);
        }

        let mut packaged_states = vec![self.prev_merkle_state()];
        let mut new_mem_block = MemBlock {
            block_producer: self.block_producer.clone(),
            block_info: self.block_info.clone(),
            prev_merkle_state: self.prev_merkle_state.clone(),
            ..Default::default()
        };

        assert!(new_mem_block.state_checkpoints.is_empty());
        assert!(new_mem_block.withdrawals.is_empty());
        assert!(new_mem_block.deposits.is_empty());
        assert!(new_mem_block.txs.is_empty());
        assert!(new_mem_block.touched_keys.is_empty());
        assert!(new_mem_block.withdrawal_post_states.is_empty());
        assert!(new_mem_block.deposit_post_states.is_empty());
        assert!(new_mem_block.tx_post_states.is_empty());
        assert!(new_mem_block.withdrawal_touched_keys_vec.is_empty());
        assert!(new_mem_block.deposit_touched_keys_vec.is_empty());

        for ((hash, touched_keys), post_state) in { self.withdrawals.iter() }
            .zip(self.withdrawal_touched_keys_vec.iter())
            .zip(self.withdrawal_post_states.iter())
            .take(withdrawals_count)
        {
            new_mem_block.push_withdrawal(*hash, post_state.clone(), touched_keys.clone());
            packaged_states.push(post_state);
        }

        let deposits = self.deposits.iter().take(deposits_count).cloned();
        let deposit_post_states = self.deposit_post_states.iter().take(deposits_count);
        let deposit_touched_keys_vec =
            { self.deposit_touched_keys_vec.iter().take(deposits_count) }.cloned();

        packaged_states.extend(deposit_post_states.clone().collect::<Vec<_>>());
        let txs_prev_state_checkpoint = {
            // Always havs prev_merkle_state, it's safe to unwrap
            let state = packaged_states.last().unwrap();
            calculate_state_checkpoint(&state.merkle_root().unpack(), state.count().unpack())
        };
        new_mem_block.push_deposits(
            deposits.collect(),
            deposit_post_states.cloned().collect(),
            deposit_touched_keys_vec.collect(),
            txs_prev_state_checkpoint,
        );

        for (hash, post_state) in { self.txs.iter() }
            .zip(self.tx_post_states.iter())
            .take(txs_count)
        {
            new_mem_block.push_tx(*hash, post_state.clone());
            packaged_states.push(post_state);
        }

        // Always havs prev_merkle_state, it's safe to unwrap
        let post_state = (*packaged_states.last().unwrap()).to_owned();

        (new_mem_block, post_state)
    }

    pub(crate) fn pack_compact(&self) -> packed::CompactMemBlock {
        packed::CompactMemBlock::new_builder()
            .txs(self.txs.pack())
            .withdrawals(self.withdrawals.pack())
            .deposits(self.deposits.pack())
            .build()
    }

    // Output diff for debug
    #[cfg(test)]
    pub(crate) fn cmp(&self, other: &MemBlock) -> MemBlockCmp {
        use MemBlockCmp::*;

        if self.block_producer != other.block_producer {
            return Diff("block producer");
        }

        if self.txs != other.txs {
            return Diff("txs");
        }

        if self.txs_set != other.txs_set {
            return Diff("txs set");
        }

        if self.withdrawals != other.withdrawals {
            return Diff("withdrawals");
        }

        if self.withdrawals_set != other.withdrawals_set {
            return Diff("withdrawals set");
        }

        if self.deposits.pack().as_slice() != other.deposits.pack().as_slice() {
            return Diff("deposits ");
        }

        if self.state_checkpoints != other.state_checkpoints {
            return Diff("state checkpoints");
        }

        if self.txs_prev_state_checkpoint != other.txs_prev_state_checkpoint {
            return Diff("txs prev state checkpoint");
        }

        if self.block_info.as_slice() != other.block_info.as_slice() {
            return Diff("block info");
        }

        if self.prev_merkle_state.as_slice() != other.prev_merkle_state.as_slice() {
            return Diff("prev merkle state");
        }

        if self.touched_keys != other.touched_keys {
            return Diff("touched keys");
        }

        if self.withdrawal_post_states.clone().pack().as_slice()
            != other.withdrawal_post_states.clone().pack().as_slice()
        {
            return Diff("withdrawal merkle_states");
        }

        if self.deposit_post_states.clone().pack().as_slice()
            != other.deposit_post_states.clone().pack().as_slice()
        {
            return Diff("deposit merkle_states");
        }

        if self.tx_post_states.clone().pack().as_slice()
            != other.tx_post_states.clone().pack().as_slice()
        {
            return Diff("tx merkle_states");
        }

        if self.withdrawal_touched_keys_vec != other.withdrawal_touched_keys_vec {
            return Diff("withdrawal touched keys vec");
        }

        if self.deposit_touched_keys_vec != other.deposit_touched_keys_vec {
            return Diff("deposit touched keys vec");
        }

        Same
    }
}

#[cfg(test)]
#[derive(Debug, PartialEq, Eq)]
pub enum MemBlockCmp {
    Same,
    Diff(&'static str),
}

#[cfg(test)]
mod test {
    use gw_common::merkle_utils::calculate_state_checkpoint;
    use gw_common::registry_address::RegistryAddress;
    use gw_common::H256;
    use gw_types::packed::{AccountMerkleState, BlockInfo};
    use gw_types::prelude::{Builder, Entity, Pack, Unpack};

    use super::MemBlock;

    #[test]
    #[should_panic]
    fn test_push_deposit_withdrawal_wrong_txs_prev_state_checkpoint() {
        let block_info = {
            let address = RegistryAddress::default();
            BlockInfo::new_builder()
                .block_producer(address.to_bytes().pack())
                .build()
        };
        let prev_merkle_state = AccountMerkleState::new_builder().count(3u32.pack()).build();

        let mut mem_block = MemBlock::new(block_info, prev_merkle_state);
        mem_block.push_deposits(
            vec![Default::default()],
            vec![random_state()],
            vec![vec![random_hash()]],
            random_hash(),
        );
    }

    #[test]
    #[should_panic]
    fn test_repackage_drop_deposits_but_not_txs() {
        let mut mem_block = MemBlock::default();

        {
            let state = random_state();
            let txs_prev_state_checkpoint =
                calculate_state_checkpoint(&state.merkle_root().unpack(), state.count().unpack());
            mem_block.push_deposits(
                vec![Default::default()],
                vec![state],
                vec![vec![random_hash()]],
                txs_prev_state_checkpoint,
            );
        }

        mem_block.push_tx(random_hash(), random_state());

        // Should drop tx first
        mem_block.repackage(0, 0, 1);
    }

    #[test]
    #[should_panic]
    fn test_repackage_drop_withdrawals_but_not_txs() {
        let mut mem_block = MemBlock::default();

        mem_block.push_withdrawal(random_hash(), random_state(), vec![random_hash()]);
        mem_block.push_tx(random_hash(), random_state());

        // Should drop tx first
        mem_block.repackage(0, 0, 1);
    }

    #[test]
    #[should_panic]
    fn test_repackage_drop_withdrawals_but_not_deposits() {
        let mut mem_block = MemBlock::default();

        mem_block.push_withdrawal(random_hash(), random_state(), vec![random_hash()]);

        {
            let state = random_state();
            let txs_prev_state_checkpoint =
                calculate_state_checkpoint(&state.merkle_root().unpack(), state.count().unpack());
            mem_block.push_deposits(
                vec![Default::default()],
                vec![state],
                vec![vec![random_hash()]],
                txs_prev_state_checkpoint,
            );
        }

        // Should drop deposit first
        mem_block.repackage(0, 1, 0);
    }

    fn random_hash() -> H256 {
        rand::random::<[u8; 32]>().into()
    }

    fn random_state() -> AccountMerkleState {
        AccountMerkleState::new_builder()
            .merkle_root(random_hash().pack())
            .count(rand::random::<u32>().pack())
            .build()
    }
}
