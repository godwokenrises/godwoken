#![allow(clippy::mutable_key_type)]

use crate::smt::smt_store::{SMTBlockStore, SMTRevertedBlockStore, SMTStateStore};
use crate::traits::chain_store::ChainStore;
use crate::traits::kv_store::KVStoreRead;
use crate::traits::kv_store::{KVStore, KVStoreWrite};
use gw_common::h256_ext::H256Ext;
use gw_common::{merkle_utils::calculate_state_checkpoint, smt::SMT, H256};
use gw_db::schema::{
    Col, COLUMN_ASSET_SCRIPT, COLUMN_BAD_BLOCK, COLUMN_BAD_BLOCK_CHALLENGE_TARGET, COLUMN_BLOCK,
    COLUMN_BLOCK_DEPOSIT_INFO_VEC, COLUMN_BLOCK_GLOBAL_STATE,
    COLUMN_BLOCK_POST_FINALIZED_CUSTODIAN_CAPACITY, COLUMN_BLOCK_SUBMIT_TX,
    COLUMN_BLOCK_SUBMIT_TX_HASH, COLUMN_INDEX, COLUMN_MEM_POOL_TRANSACTION,
    COLUMN_MEM_POOL_TRANSACTION_RECEIPT, COLUMN_MEM_POOL_WITHDRAWAL, COLUMN_META,
    COLUMN_REVERTED_BLOCK_SMT_LEAF, COLUMN_REVERTED_BLOCK_SMT_ROOT, COLUMN_TRANSACTION,
    COLUMN_TRANSACTION_INFO, COLUMN_TRANSACTION_RECEIPT, COLUMN_WITHDRAWAL, COLUMN_WITHDRAWAL_INFO,
    META_BLOCK_SMT_ROOT_KEY, META_CHAIN_ID_KEY, META_LAST_CONFIRMED_BLOCK_NUMBER_HASH_KEY,
    META_LAST_SUBMITTED_BLOCK_NUMBER_HASH_KEY, META_LAST_VALID_TIP_BLOCK_HASH_KEY,
    META_REVERTED_BLOCK_SMT_ROOT_KEY, META_TIP_BLOCK_HASH_KEY,
};
use gw_db::{error::Error, iter::DBIter, DBIterator, IteratorMode, RocksDBTransaction};
use gw_types::packed::NumberHash;
use gw_types::{
    from_box_should_be_ok,
    packed::{
        self, AccountMerkleState, Byte32, ChallengeTarget, Script, TransactionKey, WithdrawalKey,
        WithdrawalReceipt,
    },
    prelude::*,
};
use std::collections::HashSet;

pub struct StoreTransaction {
    pub(crate) inner: RocksDBTransaction,
}

impl KVStoreRead for &StoreTransaction {
    fn get(&self, col: Col, key: &[u8]) -> Option<Box<[u8]>> {
        self.inner
            .get(col, key)
            .expect("db operation should be ok")
            .map(|v| Box::<[u8]>::from(v.as_ref()))
    }
}

impl KVStoreWrite for &StoreTransaction {
    fn insert_raw(&self, col: Col, key: &[u8], value: &[u8]) -> Result<(), Error> {
        self.inner.put(col, key, value)
    }

    fn delete(&self, col: Col, key: &[u8]) -> Result<(), Error> {
        self.inner.delete(col, key)
    }
}
impl KVStore for &StoreTransaction {}
impl ChainStore for &StoreTransaction {}

impl StoreTransaction {
    pub fn commit(&self) -> Result<(), Error> {
        self.inner.commit()
    }

    pub fn rollback(&self) -> Result<(), Error> {
        self.inner.rollback()
    }

    pub(crate) fn get_iter(&self, col: Col, mode: IteratorMode) -> DBIter {
        self.inner
            .iter(col, mode)
            .expect("db operation should be ok")
    }

    pub fn setup_chain_id(&self, chain_id: H256) -> Result<(), Error> {
        self.insert_raw(COLUMN_META, META_CHAIN_ID_KEY, chain_id.as_slice())?;
        Ok(())
    }

    pub fn set_block_smt_root(&self, root: H256) -> Result<(), Error> {
        self.insert_raw(COLUMN_META, META_BLOCK_SMT_ROOT_KEY, root.as_slice())?;
        Ok(())
    }

    pub fn set_tip_block_hash(&self, block_hash: H256) -> Result<(), Error> {
        let block_hash: [u8; 32] = block_hash.into();
        self.insert_raw(COLUMN_META, META_TIP_BLOCK_HASH_KEY, &block_hash)
    }

    pub fn set_bad_block_challenge_target(
        &self,
        block_hash: &H256,
        target: &ChallengeTarget,
    ) -> Result<(), Error> {
        self.insert_raw(
            COLUMN_BAD_BLOCK_CHALLENGE_TARGET,
            block_hash.as_slice(),
            target.as_slice(),
        )
    }

    pub fn delete_bad_block_challenge_target(&self, block_hash: &H256) -> Result<(), Error> {
        self.delete(COLUMN_BAD_BLOCK_CHALLENGE_TARGET, block_hash.as_slice())
    }

    pub fn set_reverted_block_hashes(
        &self,
        reverted_block_smt_root: &H256,
        prev_reverted_block_smt_root: H256,
        mut block_hashes: Vec<H256>,
    ) -> Result<(), Error> {
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
        &self,
        block: packed::L2Block,
        global_state: packed::GlobalState,
        withdrawal_receipts: Vec<WithdrawalReceipt>,
        prev_txs_state: AccountMerkleState,
        tx_receipts: Vec<packed::TxReceipt>,
        deposit_info_vec: packed::DepositInfoVec,
        withdrawals: Vec<packed::WithdrawalRequestExtra>,
    ) -> Result<(), Error> {
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
                return Err(Error::from(format!(
                    "unexpected prev tx state, checkpoint: {} prev_txs_state_checkpoint: {}",
                    checkpoint, prev_txs_state_checkpoint
                )));
            }

            let block_post_state = block.as_reader().raw().post_account();
            if tx_receipts.is_empty() && prev_txs_state.as_slice() != block_post_state.as_slice() {
                log::debug!(
                    "tx_receipts: {} prev_txs_state: {} post_state: {}",
                    tx_receipts.len(),
                    prev_txs_state,
                    block_post_state
                );
                return Err(Error::from("unexpected no tx post state".to_string()));
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

        let post_states: Vec<AccountMerkleState> = {
            let withdrawal_post_states = withdrawal_receipts.into_iter().map(|w| w.post_state());
            let tx_post_states = tx_receipts.iter().map(|t| t.post_state());
            withdrawal_post_states.chain(tx_post_states).collect()
        };

        let state_checkpoint_list = block.raw().state_checkpoint_list().into_iter();
        if post_states.len() != state_checkpoint_list.len() {
            return Err(Error::from("unexpected block post state length".to_owned()));
        }

        Ok(())
    }

    pub fn insert_asset_scripts(&self, scripts: HashSet<Script>) -> Result<(), Error> {
        for script in scripts.into_iter() {
            self.insert_raw(COLUMN_ASSET_SCRIPT, &script.hash(), script.as_slice())?;
        }

        Ok(())
    }

    pub fn block_smt(&self) -> Result<SMT<SMTBlockStore<&Self>>, Error> {
        SMTBlockStore::new(self)
            .to_smt()
            .map_err(|err| Error::from(err.to_string()))
    }

    pub fn reverted_block_smt(&self) -> Result<SMT<SMTRevertedBlockStore<&Self>>, Error> {
        SMTRevertedBlockStore::new(self)
            .to_smt()
            .map_err(|err| Error::from(err.to_string()))
    }

    // TODO: prune db state
    pub fn get_reverted_block_hashes(&self) -> Result<HashSet<H256>, Error> {
        let iter = self.get_iter(COLUMN_REVERTED_BLOCK_SMT_LEAF, IteratorMode::End);
        let to_h256 = iter.map(|(key, _value)| {
            packed::Byte32Reader::from_slice_should_be_ok(key.as_ref()).unpack()
        });

        Ok(to_h256.collect())
    }

    pub fn rewind_reverted_block_smt(&self, block_hashes: Vec<H256>) -> Result<(), Error> {
        let mut reverted_block_smt = self.reverted_block_smt()?;

        for block_hash in block_hashes.into_iter() {
            reverted_block_smt
                .update(block_hash, H256::zero())
                .map_err(|err| Error::from(format!("reset reverted block smt error {}", err)))?;
        }

        self.set_reverted_block_smt_root(*reverted_block_smt.root())
    }

    pub fn rewind_block_smt(&self, block: &packed::L2Block) -> Result<(), Error> {
        let mut block_smt = self.block_smt()?;
        block_smt
            .update(block.smt_key().into(), H256::zero())
            .map_err(|err| Error::from(format!("reset block smt error {}", err)))?;

        self.set_block_smt_root(*block_smt.root())
    }

    fn set_last_valid_tip_block_hash(&self, block_hash: &H256) -> Result<(), Error> {
        self.insert_raw(
            COLUMN_META,
            META_LAST_VALID_TIP_BLOCK_HASH_KEY,
            block_hash.as_slice(),
        )
    }

    pub fn set_last_confirmed_block_number_hash(
        &self,
        number_hash: &packed::NumberHashReader,
    ) -> Result<(), Error> {
        self.insert_raw(
            COLUMN_META,
            META_LAST_CONFIRMED_BLOCK_NUMBER_HASH_KEY,
            number_hash.as_slice(),
        )
    }

    pub fn set_last_submitted_block_number_hash(
        &self,
        number_hash: &packed::NumberHashReader,
    ) -> Result<(), Error> {
        self.insert_raw(
            COLUMN_META,
            META_LAST_SUBMITTED_BLOCK_NUMBER_HASH_KEY,
            number_hash.as_slice(),
        )
    }

    pub fn set_block_submit_tx(
        &self,
        block_number: u64,
        tx: &packed::TransactionReader,
    ) -> Result<(), Error> {
        let k = block_number.to_be_bytes();
        self.insert_raw(COLUMN_BLOCK_SUBMIT_TX, &k, tx.as_slice())?;
        self.insert_raw(COLUMN_BLOCK_SUBMIT_TX_HASH, &k, &tx.hash())?;
        Ok(())
    }

    pub fn set_block_submit_tx_hash(
        &self,
        block_number: u64,
        hash: &[u8; 32],
    ) -> Result<(), Error> {
        let k = block_number.to_be_bytes();
        self.insert_raw(COLUMN_BLOCK_SUBMIT_TX_HASH, &k, hash)?;
        Ok(())
    }

    pub fn delete_submit_tx(&self, block_number: u64) -> Result<(), Error> {
        let k = block_number.to_be_bytes();
        self.delete(COLUMN_BLOCK_SUBMIT_TX, &k)?;
        self.delete(COLUMN_BLOCK_SUBMIT_TX_HASH, &k)
    }

    pub fn set_block_deposit_info_vec(
        &self,
        block_number: u64,
        deposit_info_vec: &packed::DepositInfoVecReader,
    ) -> Result<(), Error> {
        self.insert_raw(
            COLUMN_BLOCK_DEPOSIT_INFO_VEC,
            &block_number.to_be_bytes(),
            deposit_info_vec.as_slice(),
        )?;
        Ok(())
    }

    pub fn delete_block_deposit_info_vec(&self, block_number: u64) -> Result<(), Error> {
        self.delete(COLUMN_BLOCK_DEPOSIT_INFO_VEC, &block_number.to_be_bytes())
    }

    pub fn set_block_post_finalized_custodian_capacity(
        &self,
        block_number: u64,
        finalized_custodian_capacity: &packed::FinalizedCustodianCapacityReader,
    ) -> Result<(), Error> {
        self.insert_raw(
            COLUMN_BLOCK_POST_FINALIZED_CUSTODIAN_CAPACITY,
            &block_number.to_be_bytes(),
            finalized_custodian_capacity.as_slice(),
        )?;
        Ok(())
    }

    pub fn delete_block_post_finalized_custodian_capacity(
        &self,
        block_number: u64,
    ) -> Result<(), Error> {
        self.delete(
            COLUMN_BLOCK_POST_FINALIZED_CUSTODIAN_CAPACITY,
            &block_number.to_be_bytes(),
        )
    }

    pub fn set_reverted_block_smt_root(&self, root: H256) -> Result<(), Error> {
        self.insert_raw(
            COLUMN_META,
            META_REVERTED_BLOCK_SMT_ROOT_KEY,
            root.as_slice(),
        )?;
        Ok(())
    }

    pub fn insert_bad_block(
        &self,
        block: &packed::L2Block,
        global_state: &packed::GlobalState,
    ) -> Result<(), Error> {
        let block_hash = block.hash();

        let global_state = global_state.as_slice();

        self.insert_raw(COLUMN_BLOCK_GLOBAL_STATE, &block_hash, global_state)?;

        self.insert_raw(COLUMN_BAD_BLOCK, &block_hash, block.as_slice())?;

        // We add all block that submitted to layer-1 to block smt, even a bad block
        let mut block_smt = self.block_smt()?;
        block_smt
            .update(block.smt_key().into(), block_hash.into())
            .map_err(|err| Error::from(format!("block smt error {}", err)))?;
        self.set_block_smt_root(*block_smt.root())?;

        // Update tip block
        self.insert_raw(COLUMN_META, META_TIP_BLOCK_HASH_KEY, &block_hash)?;

        Ok(())
    }

    /// Delete bad block and block global state.
    pub fn delete_bad_block(&self, block_hash: &H256) -> Result<(), Error> {
        self.delete(COLUMN_BAD_BLOCK, block_hash.as_slice())?;
        self.delete(COLUMN_BLOCK_GLOBAL_STATE, block_hash.as_slice())?;
        Ok(())
    }

    pub fn revert_bad_blocks(&self, bad_blocks: &[packed::L2Block]) -> Result<(), Error> {
        if bad_blocks.is_empty() {
            return Ok(());
        }

        let mut block_smt = self.block_smt()?;
        let mut reverted_block_smt = self.reverted_block_smt()?;

        for block in bad_blocks {
            let block_hash = block.hash();

            // Remove block from smt
            block_smt
                .update(block.smt_key().into(), H256::zero())
                .map_err(|err| Error::from(format!("block smt error {}", err)))?;

            // Add block to reverted smt
            reverted_block_smt
                .update(block_hash.into(), H256::one())
                .map_err(|err| Error::from(format!("reverted block smt error {}", err)))?;
        }

        self.set_block_smt_root(*block_smt.root())?;
        self.set_reverted_block_smt_root(*reverted_block_smt.root())?;

        // Revert tip block to parent block
        let parent_block_hash: [u8; 32] = {
            let first_bad_block = bad_blocks.first().expect("exists");
            first_bad_block.raw().parent_block_hash().unpack()
        };
        self.insert_raw(COLUMN_META, META_TIP_BLOCK_HASH_KEY, &parent_block_hash)
    }

    /// Attach block to the rollup main chain
    pub fn attach_block(&self, block: packed::L2Block) -> Result<(), Error> {
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
            .map_err(|err| Error::from(format!("SMT error {}", err)))?;
        let root = block_smt.root();
        self.set_block_smt_root(*root)?;

        // update tip
        self.insert_raw(COLUMN_META, META_TIP_BLOCK_HASH_KEY, &block_hash)?;
        self.set_last_valid_tip_block_hash(&block_hash.into())?;

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
    pub fn detach_block(&self, block: &packed::L2Block) -> Result<(), Error> {
        // check
        {
            let tip = self.get_last_valid_tip_block_hash()?;
            assert_eq!(tip, H256::from(block.raw().hash()), "Must detach from tip");
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

        let block_hash: H256 = block.hash().into();

        // remove index
        let block_number = block.raw().number();
        self.delete(COLUMN_INDEX, block_number.as_slice())?;
        self.delete(COLUMN_INDEX, block_hash.as_slice())?;

        // update block tree
        let mut block_smt = self.block_smt()?;
        block_smt
            .update(block.smt_key().into(), H256::zero())
            .map_err(|err| Error::from(format!("SMT error {}", err)))?;
        let root = block_smt.root();
        self.set_block_smt_root(*root)?;

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
    pub fn state_smt(&self) -> Result<SMT<SMTStateStore<&Self>>, Error> {
        SMTStateStore::new(self)
            .to_smt()
            .map_err(|err| Error::from(err.to_string()))
    }

    pub fn state_smt_with_merkle_state(
        &self,
        merkle_state: AccountMerkleState,
    ) -> Result<SMT<SMTStateStore<&Self>>, Error> {
        let store = SMTStateStore::new(self);
        Ok(SMT::new(merkle_state.merkle_root().unpack(), store))
    }

    pub fn insert_mem_pool_transaction(
        &self,
        tx_hash: &H256,
        tx: packed::L2Transaction,
    ) -> Result<(), Error> {
        self.insert_raw(
            COLUMN_MEM_POOL_TRANSACTION,
            tx_hash.as_slice(),
            tx.as_slice(),
        )
    }

    pub fn remove_mem_pool_transaction(&self, tx_hash: &H256) -> Result<(), Error> {
        self.delete(COLUMN_MEM_POOL_TRANSACTION, tx_hash.as_slice())?;
        self.delete(COLUMN_MEM_POOL_TRANSACTION_RECEIPT, tx_hash.as_slice())?;
        Ok(())
    }

    pub fn insert_mem_pool_transaction_receipt(
        &self,
        tx_hash: &H256,
        tx_receipt: packed::TxReceipt,
    ) -> Result<(), Error> {
        self.insert_raw(
            COLUMN_MEM_POOL_TRANSACTION_RECEIPT,
            tx_hash.as_slice(),
            tx_receipt.as_slice(),
        )
    }

    pub fn insert_mem_pool_withdrawal(
        &self,
        withdrawal_hash: &H256,
        withdrawal: packed::WithdrawalRequestExtra,
    ) -> Result<(), Error> {
        self.insert_raw(
            COLUMN_MEM_POOL_WITHDRAWAL,
            withdrawal_hash.as_slice(),
            withdrawal.as_slice(),
        )
    }

    pub fn remove_mem_pool_withdrawal(&self, withdrawal_hash: &H256) -> Result<(), Error> {
        self.delete(COLUMN_MEM_POOL_WITHDRAWAL, withdrawal_hash.as_slice())?;
        Ok(())
    }

    pub fn get_mem_pool_withdrawal_iter(
        &self,
    ) -> impl Iterator<Item = (H256, packed::WithdrawalRequestExtra)> + '_ {
        self.get_iter(COLUMN_MEM_POOL_WITHDRAWAL, IteratorMode::End)
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
        self.get_iter(COLUMN_MEM_POOL_TRANSACTION, IteratorMode::End)
            .map(|(key, val)| {
                (
                    packed::Byte32Reader::from_slice_should_be_ok(key.as_ref()).unpack(),
                    from_box_should_be_ok!(packed::L2TransactionReader, val),
                )
            })
    }
}
