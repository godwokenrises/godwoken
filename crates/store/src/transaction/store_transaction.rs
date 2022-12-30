#![allow(clippy::mutable_key_type)]

use std::collections::HashSet;

use anyhow::{bail, Context, Result};
use autorocks::moveit::slot;
use autorocks::{DbIterator, Direction};
use gw_common::merkle_utils::calculate_state_checkpoint;
use gw_smt::smt_h256_ext::SMTH256Ext;
use gw_smt::{smt::SMT, smt_h256_ext::SMTH256};
use gw_types::packed::NumberHash;
use gw_types::{
    from_box_should_be_ok,
    h256::H256,
    packed::{
        self, AccountMerkleState, Byte32, ChallengeTarget, Script, TransactionKey, WithdrawalKey,
    },
    prelude::*,
};

use crate::schema::*;
use crate::smt::smt_store::{SMTBlockStore, SMTRevertedBlockStore, SMTStateStore};
use crate::traits::chain_store::ChainStore;
use crate::traits::kv_store::KVStoreRead;
use crate::traits::kv_store::{KVStore, KVStoreWrite};

use super::TransactionSnapshot;

pub struct StoreTransaction {
    pub(crate) inner: autorocks::Transaction,
}

impl KVStoreRead for StoreTransaction {
    fn get(&self, col: Col, key: &[u8]) -> Option<Box<[u8]>> {
        slot!(slice);
        self.inner
            .get(col, key, slice)
            .expect("db operation should be ok")
            .map(|p| p.as_ref().into())
    }
}

impl KVStoreWrite for StoreTransaction {
    fn insert_raw(&mut self, col: Col, key: &[u8], value: &[u8]) -> Result<()> {
        Ok(self.inner.put(col, key, value)?)
    }

    fn delete(&mut self, col: Col, key: &[u8]) -> Result<()> {
        Ok(self.inner.delete(col, key)?)
    }
}
impl KVStore for StoreTransaction {}
impl ChainStore for StoreTransaction {}

impl StoreTransaction {
    pub fn commit(&mut self) -> Result<()> {
        self.inner.commit()?;
        Ok(())
    }

    pub fn rollback(&mut self) -> Result<()> {
        self.inner.rollback()?;
        Ok(())
    }

    pub fn snapshot(&self) -> TransactionSnapshot {
        TransactionSnapshot {
            inner: self.inner.timestamped_snapshot(),
        }
    }

    pub(crate) fn get_iter(
        &self,
        col: Col,
        dir: Direction,
    ) -> DbIterator<&'_ autorocks::Transaction> {
        self.inner.iter(col, dir)
    }

    pub fn setup_chain_id(&mut self, chain_id: H256) -> Result<()> {
        self.insert_raw(COLUMN_META, META_CHAIN_ID_KEY, chain_id.as_slice())?;
        Ok(())
    }

    pub fn set_block_smt_root(&mut self, root: H256) -> Result<()> {
        self.insert_raw(COLUMN_META, META_BLOCK_SMT_ROOT_KEY, root.as_slice())?;
        Ok(())
    }

    pub fn set_tip_block_hash(&mut self, block_hash: H256) -> Result<()> {
        self.insert_raw(COLUMN_META, META_TIP_BLOCK_HASH_KEY, &block_hash)
    }

    pub fn set_bad_block_challenge_target(
        &mut self,
        block_hash: &H256,
        target: &ChallengeTarget,
    ) -> Result<()> {
        self.insert_raw(
            COLUMN_BAD_BLOCK_CHALLENGE_TARGET,
            block_hash.as_slice(),
            target.as_slice(),
        )
    }

    pub fn delete_bad_block_challenge_target(&mut self, block_hash: &H256) -> Result<()> {
        self.delete(COLUMN_BAD_BLOCK_CHALLENGE_TARGET, block_hash.as_slice())
    }

    pub fn set_reverted_block_hashes(
        &mut self,
        reverted_block_smt_root: &H256,
        prev_reverted_block_smt_root: H256,
        mut block_hashes: Vec<H256>,
    ) -> Result<()> {
        assert!(!block_hashes.is_empty(), "set empty reverted block hashes");

        // Prefix block hashes with prev smt root, order of origin block hashes isn't a matter.
        block_hashes.push(prev_reverted_block_smt_root);
        let last_hash_idx = block_hashes.len().saturating_sub(1);
        block_hashes.swap(0, last_hash_idx);

        self.insert_raw(
            COLUMN_REVERTED_BLOCK_SMT_ROOT,
            reverted_block_smt_root.as_slice(),
            block_hashes.pack().as_slice(),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn insert_block(
        &mut self,
        block: packed::L2Block,
        global_state: packed::GlobalState,
        prev_txs_state: AccountMerkleState,
        tx_receipts: Vec<packed::TxReceipt>,
        deposit_info_vec: packed::DepositInfoVec,
        withdrawals: Vec<packed::WithdrawalRequestExtra>,
    ) -> Result<()> {
        debug_assert_eq!(block.transactions().len(), tx_receipts.len());
        debug_assert_eq!(block.withdrawals().len(), withdrawals.len());
        let block_hash = block.hash();
        self.insert_raw(COLUMN_BLOCK, &block_hash, block.as_slice())?;
        self.insert_raw(
            COLUMN_BLOCK_GLOBAL_STATE,
            &block_hash,
            global_state.as_slice(),
        )?;
        self.set_block_deposit_info_vec(
            block.raw().number().unpack(),
            &deposit_info_vec.as_reader(),
        )?;

        // Verify prev tx state and insert
        {
            let prev_txs_state_checkpoint = {
                let txs = block.as_reader().raw().submit_transactions();
                txs.prev_state_checkpoint().to_entity()
            };

            let root = prev_txs_state.merkle_root();
            let count: u32 = prev_txs_state.count().unpack();
            let checkpoint: Byte32 = calculate_state_checkpoint(&root.unpack(), count).pack();
            if checkpoint != prev_txs_state_checkpoint {
                log::debug!("root: {} count: {}", root, count);
                bail!(
                    "unexpected prev tx state, checkpoint: {} prev_txs_state_checkpoint: {}",
                    checkpoint,
                    prev_txs_state_checkpoint
                );
            }

            let block_post_state = block.as_reader().raw().post_account();
            if tx_receipts.is_empty() && prev_txs_state.as_slice() != block_post_state.as_slice() {
                log::debug!(
                    "tx_receipts: {} prev_txs_state: {} post_state: {}",
                    tx_receipts.len(),
                    prev_txs_state,
                    block_post_state
                );
                bail!("unexpected no tx post state");
            }
        }

        for (index, (tx, tx_receipt)) in block
            .transactions()
            .into_iter()
            .zip(tx_receipts.iter())
            .enumerate()
        {
            let key = TransactionKey::build_transaction_key(block_hash.pack(), index as u32);
            self.insert_raw(COLUMN_TRANSACTION, key.as_slice(), tx.as_slice())?;
            self.insert_raw(
                COLUMN_TRANSACTION_RECEIPT,
                key.as_slice(),
                tx_receipt.as_slice(),
            )?;
        }
        for (index, withdrawal) in withdrawals.into_iter().enumerate() {
            let key = WithdrawalKey::build_withdrawal_key(block_hash.pack(), index as u32);
            self.insert_raw(COLUMN_WITHDRAWAL, key.as_slice(), withdrawal.as_slice())?;
        }

        Ok(())
    }

    pub fn insert_asset_scripts(&mut self, scripts: HashSet<Script>) -> Result<()> {
        for script in scripts.into_iter() {
            self.insert_raw(COLUMN_ASSET_SCRIPT, &script.hash(), script.as_slice())?;
        }

        Ok(())
    }

    pub fn block_smt(&mut self) -> Result<SMT<SMTBlockStore<&mut Self>>> {
        SMTBlockStore::new(self).to_smt()
    }

    pub fn reverted_block_smt(&mut self) -> Result<SMT<SMTRevertedBlockStore<&mut Self>>> {
        SMTRevertedBlockStore::new(self).to_smt()
    }

    // TODO: prune db state
    pub fn get_reverted_block_hashes(&self) -> Result<HashSet<H256>> {
        let iter = self.get_iter(COLUMN_REVERTED_BLOCK_SMT_LEAF, Direction::Backward);
        let to_h256 = iter.map(|(key, _value)| {
            packed::Byte32Reader::from_slice_should_be_ok(key.as_ref()).unpack()
        });

        Ok(to_h256.collect())
    }

    pub fn rewind_reverted_block_smt(&mut self, block_hashes: Vec<H256>) -> Result<()> {
        let mut reverted_block_smt = self.reverted_block_smt()?;

        for block_hash in block_hashes.into_iter() {
            reverted_block_smt
                .update(block_hash.into(), SMTH256::zero())
                .context("reset reverted block smt")?;
        }

        let root = *reverted_block_smt.root();
        self.set_reverted_block_smt_root(root.into())
    }

    pub fn rewind_block_smt(&mut self, block: &packed::L2Block) -> Result<()> {
        let mut block_smt = self.block_smt()?;
        block_smt
            .update(block.smt_key().into(), SMTH256::zero())
            .context("reset block smt")?;

        let root = *block_smt.root();
        self.set_block_smt_root(root.into())
    }

    fn set_last_valid_tip_block_hash(&mut self, block_hash: &H256) -> Result<()> {
        self.insert_raw(
            COLUMN_META,
            META_LAST_VALID_TIP_BLOCK_HASH_KEY,
            block_hash.as_slice(),
        )
    }

    pub fn set_last_confirmed_block_number_hash(
        &mut self,
        number_hash: &packed::NumberHashReader,
    ) -> Result<()> {
        self.insert_raw(
            COLUMN_META,
            META_LAST_CONFIRMED_BLOCK_NUMBER_HASH_KEY,
            number_hash.as_slice(),
        )
    }

    pub fn set_last_submitted_block_number_hash(
        &mut self,
        number_hash: &packed::NumberHashReader,
    ) -> Result<()> {
        self.insert_raw(
            COLUMN_META,
            META_LAST_SUBMITTED_BLOCK_NUMBER_HASH_KEY,
            number_hash.as_slice(),
        )
    }

    pub fn set_block_submit_tx(
        &mut self,
        block_number: u64,
        tx: &packed::TransactionReader,
    ) -> Result<()> {
        let k = block_number.to_be_bytes();
        self.insert_raw(COLUMN_BLOCK_SUBMIT_TX, &k, tx.as_slice())?;
        self.insert_raw(COLUMN_BLOCK_SUBMIT_TX_HASH, &k, &tx.hash())?;
        Ok(())
    }

    pub fn set_block_submit_tx_hash(&mut self, block_number: u64, hash: &[u8; 32]) -> Result<()> {
        let k = block_number.to_be_bytes();
        self.insert_raw(COLUMN_BLOCK_SUBMIT_TX_HASH, &k, hash)?;
        Ok(())
    }

    pub fn delete_submit_tx(&mut self, block_number: u64) -> Result<()> {
        let k = block_number.to_be_bytes();
        self.delete(COLUMN_BLOCK_SUBMIT_TX, &k)?;
        self.delete(COLUMN_BLOCK_SUBMIT_TX_HASH, &k)
    }

    pub fn set_block_deposit_info_vec(
        &mut self,
        block_number: u64,
        deposit_info_vec: &packed::DepositInfoVecReader,
    ) -> Result<()> {
        self.insert_raw(
            COLUMN_BLOCK_DEPOSIT_INFO_VEC,
            &block_number.to_be_bytes(),
            deposit_info_vec.as_slice(),
        )?;
        Ok(())
    }

    pub fn delete_block_deposit_info_vec(&mut self, block_number: u64) -> Result<()> {
        self.delete(COLUMN_BLOCK_DEPOSIT_INFO_VEC, &block_number.to_be_bytes())
    }

    pub fn set_block_post_finalized_custodian_capacity(
        &mut self,
        block_number: u64,
        finalized_custodian_capacity: &packed::FinalizedCustodianCapacityReader,
    ) -> Result<()> {
        self.insert_raw(
            COLUMN_BLOCK_POST_FINALIZED_CUSTODIAN_CAPACITY,
            &block_number.to_be_bytes(),
            finalized_custodian_capacity.as_slice(),
        )?;
        Ok(())
    }

    pub fn delete_block_post_finalized_custodian_capacity(
        &mut self,
        block_number: u64,
    ) -> Result<()> {
        self.delete(
            COLUMN_BLOCK_POST_FINALIZED_CUSTODIAN_CAPACITY,
            &block_number.to_be_bytes(),
        )
    }

    pub fn set_reverted_block_smt_root(&mut self, root: H256) -> Result<()> {
        self.insert_raw(
            COLUMN_META,
            META_REVERTED_BLOCK_SMT_ROOT_KEY,
            root.as_slice(),
        )?;
        Ok(())
    }

    pub fn insert_bad_block(
        &mut self,
        block: &packed::L2Block,
        global_state: &packed::GlobalState,
    ) -> Result<()> {
        let block_hash = block.hash();

        let global_state = global_state.as_slice();

        self.insert_raw(COLUMN_BLOCK_GLOBAL_STATE, &block_hash, global_state)?;

        self.insert_raw(COLUMN_BAD_BLOCK, &block_hash, block.as_slice())?;

        // We add all block that submitted to layer-1 to block smt, even a bad block
        let mut block_smt = self.block_smt()?;
        block_smt
            .update(block.smt_key().into(), block_hash.into())
            .context("update block smt")?;
        let root = *block_smt.root();
        self.set_block_smt_root(root.into())?;

        // Update tip block
        self.insert_raw(COLUMN_META, META_TIP_BLOCK_HASH_KEY, &block_hash)?;

        Ok(())
    }

    /// Delete bad block and block global state.
    pub fn delete_bad_block(&mut self, block_hash: &H256) -> Result<()> {
        self.delete(COLUMN_BAD_BLOCK, block_hash.as_slice())?;
        self.delete(COLUMN_BLOCK_GLOBAL_STATE, block_hash.as_slice())?;
        Ok(())
    }

    pub fn revert_bad_blocks(&mut self, bad_blocks: &[packed::L2Block]) -> Result<()> {
        if bad_blocks.is_empty() {
            return Ok(());
        }

        let mut block_smt = self.block_smt()?;
        for block in bad_blocks {
            // Remove block from smt
            block_smt
                .update(block.smt_key().into(), SMTH256::zero())
                .context("update block smt")?;
        }
        let root = *block_smt.root();
        self.set_block_smt_root(root.into())?;

        let mut reverted_block_smt = self.reverted_block_smt()?;
        for block in bad_blocks {
            let block_hash = block.hash();
            // Add block to reverted smt
            reverted_block_smt
                .update(block_hash.into(), SMTH256::one())
                .context("update reverted block smt")?;
        }
        let root = *reverted_block_smt.root();
        self.set_reverted_block_smt_root(root.into())?;

        // Revert tip block to parent block
        let parent_block_hash: [u8; 32] = {
            let first_bad_block = bad_blocks.first().expect("exists");
            first_bad_block.raw().parent_block_hash().unpack()
        };
        self.insert_raw(COLUMN_META, META_TIP_BLOCK_HASH_KEY, &parent_block_hash)
    }

    /// Attach block to the rollup main chain
    pub fn attach_block(&mut self, block: packed::L2Block) -> Result<()> {
        let raw = block.raw();
        let raw_number = raw.number();
        let block_hash = raw.hash();

        // build tx info
        for (index, tx) in block.transactions().into_iter().enumerate() {
            let key = TransactionKey::build_transaction_key(block_hash.pack(), index as u32);
            let info = packed::TransactionInfo::new_builder()
                .key(key)
                .block_number(raw_number.clone())
                .build();
            let tx_hash = tx.hash();
            self.insert_raw(COLUMN_TRANSACTION_INFO, &tx_hash, info.as_slice())?;
        }

        // build withdrawal info
        for (index, withdrawal) in block.withdrawals().into_iter().enumerate() {
            let key = WithdrawalKey::build_withdrawal_key(block_hash.pack(), index as u32);
            let info = packed::WithdrawalInfo::new_builder()
                .key(key)
                .block_number(raw_number.clone())
                .build();
            let withdrawal_hash = withdrawal.hash();
            self.insert_raw(COLUMN_WITHDRAWAL_INFO, &withdrawal_hash, info.as_slice())?;
        }

        // build main chain index
        self.insert_raw(COLUMN_INDEX, raw_number.as_slice(), &block_hash)?;
        self.insert_raw(COLUMN_INDEX, &block_hash, raw_number.as_slice())?;

        // update block tree
        let mut block_smt = self.block_smt()?;
        block_smt
            .update(raw.smt_key().into(), raw.hash().into())
            .context("update block smt")?;
        let root = *block_smt.root();
        self.set_block_smt_root(root.into())?;

        // update tip
        self.insert_raw(COLUMN_META, META_TIP_BLOCK_HASH_KEY, &block_hash)?;
        self.set_last_valid_tip_block_hash(&block_hash)?;

        Ok(())
    }

    /// Delete block from DB
    ///
    /// Will update last confirmed / last submitted block to parent block if the
    /// current value points to the deleted block.
    ///
    /// # Panics
    ///
    /// If the block is not the “last valid tip block”.
    pub fn detach_block(&mut self, block: &packed::L2Block) -> Result<()> {
        // check
        {
            let tip = self.get_last_valid_tip_block_hash()?;
            assert_eq!(tip, block.raw().hash(), "Must detach from tip");
        }
        {
            let number: u64 = block.raw().number().unpack();
            let hash: Byte32 = block.hash().pack();
            log::warn!("detach block #{} {}", number, hash);
        }
        // remove transaction info
        for tx in block.transactions().into_iter() {
            let tx_hash = tx.hash();
            self.delete(COLUMN_TRANSACTION_INFO, &tx_hash)?;
        }
        // withdrawal info
        for withdrawal in block.withdrawals() {
            let withdrawal_hash = withdrawal.hash();
            self.delete(COLUMN_WITHDRAWAL_INFO, &withdrawal_hash)?;
        }

        let block_hash: H256 = block.hash();

        // remove index
        let block_number = block.raw().number();
        self.delete(COLUMN_INDEX, block_number.as_slice())?;
        self.delete(COLUMN_INDEX, block_hash.as_slice())?;

        // update block tree
        let mut block_smt = self.block_smt()?;
        block_smt
            .update(block.smt_key().into(), SMTH256::zero())
            .context("update block smt")?;
        let root = *block_smt.root();
        self.set_block_smt_root(root.into())?;

        // update tip
        let block_number: u64 = block_number.unpack();
        let parent_number = block_number.saturating_sub(1);
        let parent_block_hash = self
            .get_block_hash_by_number(parent_number)?
            .expect("parent block hash");
        self.insert_raw(
            COLUMN_META,
            META_TIP_BLOCK_HASH_KEY,
            parent_block_hash.as_slice(),
        )?;
        self.set_last_valid_tip_block_hash(&parent_block_hash)?;

        let parent_number_hash = NumberHash::new_builder()
            .number(parent_number.pack())
            .block_hash(parent_block_hash.pack())
            .build();
        // Update last confirmed block to parent if the current last confirmed block is this block.
        if self
            .get_last_confirmed_block_number_hash()
            .map(|nh| nh.number().unpack())
            == Some(block_number)
        {
            self.set_last_confirmed_block_number_hash(&parent_number_hash.as_reader())?;
        }
        // Update last submitted block to parent if the current last submitted block is this block.
        if self
            .get_last_submitted_block_number_hash()
            .map(|nh| nh.number().unpack())
            == Some(block_number)
        {
            self.set_last_submitted_block_number_hash(&parent_number_hash.as_reader())?;
        }

        self.delete_submit_tx(block_number)?;
        self.delete_block_deposit_info_vec(block_number)?;
        self.delete_block_post_finalized_custodian_capacity(block_number)?;

        Ok(())
    }

    // FIXME: This method may running into inconsistent state if current state is dirty.
    // We should separate the StateDB into ReadOnly & WriteOnly,
    // The ReadOnly is for fetching history state, and the write only is for writing new state.
    // This function should only be added on the ReadOnly state.
    pub fn state_smt(&mut self) -> Result<SMT<SMTStateStore<&mut Self>>> {
        SMTStateStore::new(self).to_smt()
    }

    pub fn state_smt_with_merkle_state(
        &mut self,
        merkle_state: AccountMerkleState,
    ) -> Result<SMT<SMTStateStore<&mut Self>>> {
        let store = SMTStateStore::new(self);
        let root: H256 = merkle_state.merkle_root().unpack();
        Ok(SMT::new(root.into(), store))
    }

    pub fn insert_mem_pool_transaction(
        &mut self,
        tx_hash: &H256,
        tx: packed::L2Transaction,
    ) -> Result<()> {
        self.insert_raw(
            COLUMN_MEM_POOL_TRANSACTION,
            tx_hash.as_slice(),
            tx.as_slice(),
        )
    }

    pub fn remove_mem_pool_transaction(&mut self, tx_hash: &H256) -> Result<()> {
        self.delete(COLUMN_MEM_POOL_TRANSACTION, tx_hash.as_slice())?;
        self.delete(COLUMN_MEM_POOL_TRANSACTION_RECEIPT, tx_hash.as_slice())?;
        Ok(())
    }

    pub fn insert_mem_pool_transaction_receipt(
        &mut self,
        tx_hash: &H256,
        tx_receipt: packed::TxReceipt,
    ) -> Result<()> {
        self.insert_raw(
            COLUMN_MEM_POOL_TRANSACTION_RECEIPT,
            tx_hash.as_slice(),
            tx_receipt.as_slice(),
        )
    }

    pub fn insert_mem_pool_withdrawal(
        &mut self,
        withdrawal_hash: &H256,
        withdrawal: packed::WithdrawalRequestExtra,
    ) -> Result<()> {
        self.insert_raw(
            COLUMN_MEM_POOL_WITHDRAWAL,
            withdrawal_hash.as_slice(),
            withdrawal.as_slice(),
        )
    }

    pub fn remove_mem_pool_withdrawal(&mut self, withdrawal_hash: &H256) -> Result<()> {
        self.delete(COLUMN_MEM_POOL_WITHDRAWAL, withdrawal_hash.as_slice())?;
        Ok(())
    }

    pub fn get_mem_pool_withdrawal_iter(
        &self,
    ) -> impl Iterator<Item = (H256, packed::WithdrawalRequestExtra)> + '_ {
        self.get_iter(COLUMN_MEM_POOL_WITHDRAWAL, Direction::Backward)
            .map(|(key, val)| {
                (
                    packed::Byte32Reader::from_slice_should_be_ok(key.as_ref()).unpack(),
                    from_box_should_be_ok!(packed::WithdrawalRequestExtraReader, val),
                )
            })
    }

    pub fn get_mem_pool_transaction_iter(
        &self,
    ) -> impl Iterator<Item = (H256, packed::L2Transaction)> + '_ {
        self.get_iter(COLUMN_MEM_POOL_TRANSACTION, Direction::Backward)
            .map(|(key, val)| {
                (
                    packed::Byte32Reader::from_slice_should_be_ok(key.as_ref()).unpack(),
                    from_box_should_be_ok!(packed::L2TransactionReader, val),
                )
            })
    }
}
