//! State DB

use crate::constant::MEMORY_BLOCK_NUMBER;
use crate::{smt_store_impl::SMTStore, traits::KVStore, transaction::StoreTransaction};
use anyhow::{anyhow, Result};
use gw_common::merkle_utils::calculate_state_checkpoint;
use gw_common::{error::Error as StateError, smt::SMT, state::State, H256};
use gw_db::schema::{
    Col, COLUMN_ACCOUNT_SMT_BRANCH, COLUMN_ACCOUNT_SMT_LEAF, COLUMN_DATA, COLUMN_SCRIPT,
    COLUMN_SCRIPT_PREFIX,
};
use gw_db::{error::Error, iter::DBIter, DBRawIterator, IteratorMode};
use gw_traits::CodeStore;
use gw_types::{
    bytes::Bytes,
    packed::{self, AccountMerkleState, L2Block},
    prelude::*,
};
use std::{cell::RefCell, collections::HashSet, fmt, mem::size_of_val};

const FLAG_DELETE_VALUE: u8 = 0;

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct WriteContext {
    pub tx_offset: u32,
}

impl WriteContext {
    pub fn new(tx_offset: u32) -> Self {
        Self { tx_offset }
    }
}

impl Default for WriteContext {
    fn default() -> Self {
        Self { tx_offset: 0 }
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum StateDBMode {
    Genesis,
    ReadOnly,
    Write(WriteContext),
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum SubState {
    Withdrawal(u32),
    PrevTxs,
    Tx(u32),
    Block, // Block post state
    MemBlock(u32),
}

impl SubState {
    fn validate_in_block(&self, block: &L2Block) -> Result<()> {
        match self {
            SubState::Withdrawal(index) => {
                if *index as usize >= block.withdrawals().len() {
                    return Err(anyhow!("invalid withdrawal substate index"));
                }
            }
            SubState::Tx(index) => {
                if *index as usize >= block.transactions().len() {
                    return Err(anyhow!("invalid tx substate index"));
                }
            }
            _ => (),
        }

        Ok(())
    }

    fn extract_checkpoint_post_state_from_block(
        &self,
        db: &StoreTransaction,
        block: &L2Block,
    ) -> Result<AccountMerkleState, Error> {
        self.validate_in_block(block).map_err(|e| e.to_string())?;

        enum CheckPointIdx {
            PrevTxs,
            Idx(usize),
        }

        let checkpoint_idx = match *self {
            SubState::Withdrawal(index) => CheckPointIdx::Idx(index as usize),
            SubState::PrevTxs => CheckPointIdx::PrevTxs,
            SubState::Tx(index) => CheckPointIdx::Idx(block.withdrawals().len() + index as usize),
            SubState::Block => return Ok(block.raw().post_account()),
            SubState::MemBlock(_) => {
                let root = db.get_mem_block_account_smt_root()?;
                let count = db.get_mem_block_account_count()?;
                let merkle_state = AccountMerkleState::new_builder()
                    .merkle_root(root.pack())
                    .count(count.pack())
                    .build();
                return Ok(merkle_state);
            }
        };

        let checkpoint = match checkpoint_idx {
            CheckPointIdx::Idx(idx) => {
                let checkpoints = block.as_reader().raw().state_checkpoint_list().to_entity();
                checkpoints.get(idx).expect("checkpoint exists")
            }
            CheckPointIdx::PrevTxs => {
                let txs = block.as_reader().raw().submit_transactions();
                txs.prev_state_checkpoint().to_entity()
            }
        };

        db.get_checkpoint_post_state(&checkpoint)?
            .ok_or_else(|| "can't find checkpoint".to_string().into())
    }
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct CheckPoint {
    block_number: u64,
    sub_state: SubState,
}

impl CheckPoint {
    pub fn new(block_number: u64, sub_state: SubState) -> Self {
        Self {
            block_number,
            sub_state,
        }
    }

    pub fn from_genesis() -> Self {
        Self {
            block_number: 0,
            sub_state: SubState::Block,
        }
    }

    pub fn from_block_hash(
        db: &StoreTransaction,
        block_hash: H256,
        sub_state: SubState,
    ) -> Result<Self> {
        let block = db
            .get_block(&block_hash)?
            .ok_or_else(|| anyhow!("block isn't exist"))?;

        sub_state.validate_in_block(&block)?;

        Ok(CheckPoint {
            block_number: block.raw().number().unpack(),
            sub_state,
        })
    }

    #[cfg(test)]
    pub fn do_extract_block_number_and_index_number(
        &self,
        db: &StoreTransaction,
        db_mode: StateDBMode,
    ) -> Result<(u64, u32), Error> {
        self.extract_block_number_and_index_number(db, db_mode)
    }

    fn extract_block_number_and_index_number(
        &self,
        db: &StoreTransaction,
        db_mode: StateDBMode,
    ) -> Result<(u64, u32), Error> {
        let block_offset = |withdrawal_count: u32, tx_count: u32| -> u32 {
            if 0 == withdrawal_count {
                tx_count // 0 is prev txs
            } else {
                // For example: 2 withdrawals + 2 txs, we should have 5 checkpoint, a.k.a 0..=4
                withdrawal_count + 1 + tx_count.saturating_sub(1)
            }
        };
        let tx_offset = |withdrawal_count: u32, tx_index: u32| -> u32 {
            if 0 == withdrawal_count {
                1 + tx_index
            } else {
                // For example: 0 withdrawal, then first tx should write to 1th place
                // 1 withdrawal, then first tx should write to 2th place
                withdrawal_count + 1 + tx_index
            }
        };

        match self.sub_state {
            SubState::Withdrawal(index) => Ok((self.block_number, index)),
            SubState::PrevTxs => match db_mode {
                StateDBMode::Genesis => Ok((self.block_number, 0)),
                StateDBMode::ReadOnly => {
                    let block = {
                        let block_hash = db
                            .get_block_hash_by_number(self.block_number)?
                            .ok_or_else(|| "can't find block hash".to_string())?;

                        db.get_block(&block_hash)?
                            .ok_or_else(|| "can't find block".to_string())?
                    };

                    let block = block.as_reader();
                    let prev_txs_state_checkpoint: [u8; 32] = {
                        let txs = block.raw().submit_transactions();
                        txs.prev_state_checkpoint().unpack()
                    };
                    let prev_account_checkpoint: [u8; 32] = {
                        let account = block.raw().prev_account();
                        let root = account.merkle_root().unpack();
                        let count = account.count().unpack();

                        calculate_state_checkpoint(&root, count).into()
                    };
                    if prev_txs_state_checkpoint == prev_account_checkpoint {
                        // No deposit, no withdrawal, state across block, should use
                        // previous block.
                        let prev_block = {
                            let block_hash = db
                                .get_block_hash_by_number(self.block_number.saturating_sub(1))?
                                .ok_or_else(|| "can't find block hash".to_string())?;

                            db.get_block(&block_hash)?
                                .ok_or_else(|| "can't find block".to_string())?
                        };
                        let offset = block_offset(
                            prev_block.withdrawals().len() as u32,
                            prev_block.transactions().len() as u32,
                        );

                        Ok((self.block_number.saturating_sub(1), offset))
                    } else {
                        Ok((self.block_number, block.withdrawals().len() as u32))
                    }
                }
                StateDBMode::Write(ctx) => Ok((self.block_number, ctx.tx_offset)),
            },
            SubState::Tx(index) => match db_mode {
                StateDBMode::Genesis => Ok((self.block_number, 0)),
                StateDBMode::ReadOnly => {
                    let block = {
                        let block_hash = db
                            .get_block_hash_by_number(self.block_number)?
                            .ok_or_else(|| "can't find block hash".to_string())?;

                        db.get_block(&block_hash)?
                            .ok_or_else(|| "can't find block".to_string())?
                    };

                    let offset = tx_offset(block.withdrawals().len() as u32, index);
                    Ok((self.block_number, offset))
                }
                StateDBMode::Write(ctx) => {
                    let offset = tx_offset(ctx.tx_offset, index);
                    Ok((self.block_number, offset as u32))
                }
            },
            SubState::Block => match db_mode {
                StateDBMode::Genesis => Ok((self.block_number, 0)),
                StateDBMode::ReadOnly => {
                    let block = {
                        let block_hash = db
                            .get_block_hash_by_number(self.block_number)?
                            .ok_or_else(|| "can't find block hash".to_string())?;

                        db.get_block(&block_hash)?
                            .ok_or_else(|| "can't find block".to_string())?
                    };

                    let offset = block_offset(
                        block.withdrawals().len() as u32,
                        block.transactions().len() as u32,
                    );
                    Ok((self.block_number, offset as u32))
                }
                StateDBMode::Write(_ctx) => Ok((self.block_number, 0)),
            },
            SubState::MemBlock(offset) => Ok((MEMORY_BLOCK_NUMBER, offset)),
        }
    }
}

pub struct StateDBTransaction<'db> {
    inner: &'db StoreTransaction,
    checkpoint: CheckPoint,
    mode: StateDBMode,
}

impl<'db> KVStore for StateDBTransaction<'db> {
    fn get(&self, col: Col, key: &[u8]) -> Option<Box<[u8]>> {
        let raw_key = self.get_key_with_suffix(key);
        let mut raw_iter: DBRawIterator = self.inner.get_iter(col, IteratorMode::Start).into();
        raw_iter.seek_for_prev(raw_key);
        self.filter_value_of_seek(key, &raw_iter)
    }

    // TODO: this trait method will be deleted in the future.
    fn get_iter(&self, col: Col, mode: IteratorMode) -> DBIter {
        self.inner.get_iter(col, mode)
    }

    fn insert_raw(&self, col: Col, key: &[u8], value: &[u8]) -> Result<(), Error> {
        assert_ne!(
            value,
            &FLAG_DELETE_VALUE.to_be_bytes(),
            "forbid inserting the delete flag"
        );
        let raw_key = self.get_key_with_suffix(key);
        self.inner
            .insert_raw(col, &raw_key, value)
            .and(self.record_block_state(col, &raw_key))
    }

    fn delete(&self, col: Col, key: &[u8]) -> Result<(), Error> {
        let raw_key = self.get_key_with_suffix(key);
        self.inner
            .insert_raw(col, &raw_key, &FLAG_DELETE_VALUE.to_be_bytes())
            .and(self.record_block_state(col, &raw_key))
    }
}

impl<'db> fmt::Debug for StateDBTransaction<'db> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StateDBTransaction")
            .field("checkpoint", &self.checkpoint)
            .field("mode", &self.mode)
            .finish()
    }
}

impl<'db> StateDBTransaction<'db> {
    pub fn from_checkpoint(
        inner: &'db StoreTransaction,
        checkpoint: CheckPoint,
        mode: StateDBMode,
    ) -> Result<Self, Error> {
        Ok(StateDBTransaction {
            inner,
            checkpoint,
            mode,
        })
    }

    pub fn mode(&self) -> StateDBMode {
        self.mode
    }

    pub fn commit(&self) -> Result<(), Error> {
        if self.mode == StateDBMode::ReadOnly {
            Err(Error::from("commit on ReadOnly mode".to_string()))
        } else {
            self.inner.commit()
        }
    }

    pub fn account_smt_store(&self) -> Result<SMTStore<'_, Self>, Error> {
        let smt_store = SMTStore::new(COLUMN_ACCOUNT_SMT_LEAF, COLUMN_ACCOUNT_SMT_BRANCH, self);
        Ok(smt_store)
    }

    fn account_smt_with_merkle_state(
        &self,
        merkle_state: AccountMerkleState,
    ) -> Result<SMT<SMTStore<'_, Self>>, Error> {
        let smt_store = self.account_smt_store()?;
        Ok(SMT::new(merkle_state.merkle_root().unpack(), smt_store))
    }

    pub fn state_tree_with_merkle_state(
        &self,
        merkle_state: AccountMerkleState,
    ) -> Result<StateTree<'_, 'db>, Error> {
        Ok(StateTree::new(
            self,
            self.account_smt_with_merkle_state(merkle_state.clone())?,
            merkle_state.count().unpack(),
        ))
    }

    // FIXME: This method may running into inconsistent state if current state is dirty.
    // We should separate the StateDB into ReadOnly & WriteOnly,
    // The ReadOnly is for fetching history state, and the write only is for writing new state.
    // This function should only be added on the ReadOnly state.
    pub fn account_smt(&self) -> Result<SMT<SMTStore<'_, Self>>, Error> {
        let merkle_state = self.get_checkpoint_merkle_state()?;
        self.account_smt_with_merkle_state(merkle_state)
    }

    pub fn state_tree(&self) -> Result<StateTree<'_, 'db>, Error> {
        let merkle_state = self.get_checkpoint_merkle_state()?;
        self.state_tree_with_merkle_state(merkle_state)
    }

    fn get_checkpoint_merkle_state(&self) -> Result<AccountMerkleState, Error> {
        let inner_db = self.inner;

        let account_merkle_state = match self.mode {
            StateDBMode::Genesis => {
                if 0 != self.checkpoint.block_number
                    || (SubState::Block != self.checkpoint.sub_state
                        && SubState::PrevTxs != self.checkpoint.sub_state)
                {
                    return Err(Error::from(format!(
                        "invalid check point {:?} for StateDBMode::Genesis",
                        self.checkpoint
                    )));
                }

                // NOTE: Genesis doesn't have txs, so prev txs state is same as
                // post account state.
                match self.inner.get_block_hash_by_number(0)? {
                    Some(block_hash) => {
                        let block = inner_db
                            .get_block(&block_hash)?
                            .ok_or_else(|| "can't find genesis".to_string())?;
                        block.raw().post_account()
                    }
                    None => AccountMerkleState::default(),
                }
            }
            StateDBMode::ReadOnly => {
                let block = {
                    let block_hash = inner_db
                        .get_block_hash_by_number(self.checkpoint.block_number)?
                        .ok_or_else(|| "can't find block hash".to_string())?;

                    inner_db
                        .get_block(&block_hash)?
                        .ok_or_else(|| "can't find block".to_string())?
                };

                self.checkpoint
                    .sub_state
                    .extract_checkpoint_post_state_from_block(inner_db, &block)?
            }
            StateDBMode::Write(_ctx) => {
                let mut last_block_number = self.checkpoint.block_number;
                let mut opt_block_hash = None;
                while opt_block_hash.is_none() {
                    opt_block_hash = inner_db.get_block_hash_by_number(last_block_number)?;
                    if opt_block_hash.is_none() && 0 == last_block_number {
                        panic!("genesis block not found in StateDBMode::Write");
                    }
                    last_block_number = last_block_number.saturating_sub(1);
                }

                let block = inner_db
                    .get_block(&opt_block_hash.expect("block hash exists"))?
                    .ok_or_else(|| "can't find block".to_string())?;

                let sub_state = if block.raw().number().unpack() == self.checkpoint.block_number {
                    self.checkpoint.sub_state.clone()
                } else {
                    SubState::Block
                };

                sub_state.extract_checkpoint_post_state_from_block(inner_db, &block)?
            }
        };

        Ok(account_merkle_state)
    }

    fn get_key_with_suffix(&self, key: &[u8]) -> Vec<u8> {
        let (block_number, index) = self
            .checkpoint
            .extract_block_number_and_index_number(self.inner, self.mode)
            .expect("block number and index number");

        [key, &block_number.to_be_bytes(), &index.to_be_bytes()].concat()
    }

    fn get_original_key<'a>(&self, raw_key: &'a [u8]) -> &'a [u8] {
        let (block_number, index) = self
            .checkpoint
            .extract_block_number_and_index_number(self.inner, self.mode)
            .expect("block number and index number");

        &raw_key[..raw_key.len() - size_of_val(&block_number) - size_of_val(&index)]
    }

    fn filter_value_of_seek(&self, ori_key: &[u8], raw_iter: &DBRawIterator) -> Option<Box<[u8]>> {
        if !raw_iter.valid() {
            return None;
        }
        match raw_iter.key() {
            Some(raw_key_found) => {
                if ori_key != self.get_original_key(raw_key_found) {
                    return None;
                }

                match raw_iter.value() {
                    Some(&[FLAG_DELETE_VALUE]) => None,
                    Some(value) => Some(Box::<[u8]>::from(value)),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    fn record_block_state(&self, col: Col, raw_key: &[u8]) -> Result<(), Error> {
        // skip genesis
        if self.mode == StateDBMode::Genesis {
            return Ok(());
        }

        let (block_number, index) = self
            .checkpoint
            .extract_block_number_and_index_number(self.inner, self.mode)?;

        self.inner
            .record_block_state(block_number, index, col, raw_key)?;
        Ok(())
    }
}

/// Tracker state changes
pub struct StateTracker {
    touched_keys: Option<RefCell<HashSet<H256>>>,
}

impl Default for StateTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl StateTracker {
    pub fn new() -> Self {
        StateTracker { touched_keys: None }
    }

    /// Enable state tracking
    pub fn enable(&mut self) {
        if self.touched_keys.is_none() {
            self.touched_keys = Some(Default::default())
        }
    }

    /// Return touched keys
    pub fn touched_keys(&self) -> Option<&RefCell<HashSet<H256>>> {
        self.touched_keys.as_ref()
    }

    /// Record a key in the tracker
    pub fn touch_key(&self, key: &H256) {
        if let Some(touched_keys) = self.touched_keys.as_ref() {
            touched_keys.borrow_mut().insert(*key);
        }
    }
}

pub struct StateTree<'a, 'db> {
    tree: SMT<SMTStore<'a, StateDBTransaction<'db>>>,
    account_count: u32,
    db: &'a StateDBTransaction<'db>,
    tracker: StateTracker,
}

impl<'a, 'db> StateTree<'a, 'db> {
    pub fn new(
        db: &'a StateDBTransaction<'db>,
        tree: SMT<SMTStore<'a, StateDBTransaction<'db>>>,
        account_count: u32,
    ) -> Self {
        StateTree {
            tree,
            db,
            account_count,
            tracker: StateTracker::new(),
        }
    }

    pub fn tracker_mut(&mut self) -> &mut StateTracker {
        &mut self.tracker
    }

    pub fn get_merkle_state(&self) -> AccountMerkleState {
        AccountMerkleState::new_builder()
            .merkle_root(self.tree.root().pack())
            .count(self.account_count.pack())
            .build()
    }

    /// submit tree changes into memory block
    /// notice, this function do not commit the DBTransaction
    pub fn submit_tree_to_mem_block(&self) -> Result<(), Error> {
        self.db
            .inner
            .set_mem_block_account_smt_root(*self.tree.root())
            .expect("set smt root");
        self.db
            .inner
            .set_mem_block_account_count(self.account_count)
            .expect("set smt root");
        Ok(())
    }
}

impl<'a, 'db> State for StateTree<'a, 'db> {
    fn get_raw(&self, key: &H256) -> Result<H256, StateError> {
        self.tracker.touch_key(key);
        let v = self.tree.get(key)?;
        Ok(v)
    }

    fn update_raw(&mut self, key: H256, value: H256) -> Result<(), StateError> {
        self.tracker.touch_key(&key);
        self.tree.update(key, value)?;
        Ok(())
    }

    fn get_account_count(&self) -> Result<u32, StateError> {
        Ok(self.account_count)
    }

    fn set_account_count(&mut self, count: u32) -> Result<(), StateError> {
        self.account_count = count;
        Ok(())
    }

    fn calculate_root(&self) -> Result<H256, StateError> {
        let root = self.tree.root();
        Ok(*root)
    }
}

impl<'a, 'db> CodeStore for StateTree<'a, 'db> {
    fn insert_script(&mut self, script_hash: H256, script: packed::Script) {
        self.db
            .insert_raw(COLUMN_SCRIPT, script_hash.as_slice(), script.as_slice())
            .expect("insert script");

        // build script_hash prefix search index
        self.db
            .insert_raw(
                COLUMN_SCRIPT_PREFIX,
                &script_hash.as_slice()[..20],
                script_hash.as_slice(),
            )
            .expect("insert script prefix");
    }

    fn get_script(&self, script_hash: &H256) -> Option<packed::Script> {
        self.db
            .get(COLUMN_SCRIPT, script_hash.as_slice())
            .map(|slice| packed::ScriptReader::from_slice_should_be_ok(slice.as_ref()).to_entity())
    }

    fn get_script_hash_by_short_address(&self, script_hash_prefix: &[u8]) -> Option<H256> {
        match self.db.get(COLUMN_SCRIPT_PREFIX, script_hash_prefix) {
            Some(slice) => {
                let mut hash = [0u8; 32];
                hash.copy_from_slice(slice.as_ref());
                Some(hash.into())
            }
            None => None,
        }
    }

    fn insert_data(&mut self, data_hash: H256, code: Bytes) {
        self.db
            .insert_raw(COLUMN_DATA, data_hash.as_slice(), &code)
            .expect("insert data");
    }

    fn get_data(&self, data_hash: &H256) -> Option<Bytes> {
        self.db
            .get(COLUMN_DATA, data_hash.as_slice())
            .map(|slice| Bytes::from(slice.to_vec()))
    }
}
