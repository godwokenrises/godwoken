//! State DB

use crate::{smt_store_impl::SMTStore, traits::KVStore, transaction::StoreTransaction};
use anyhow::{anyhow, Result};
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
pub enum StateDBMode {
    Genesis,
    ReadOnly,
    Write,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum SubState {
    Withdrawal(u32),
    Tx(u32),
    Block, // Block post state
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

        let checkpoint_idx = match *self {
            SubState::Withdrawal(index) => index as usize,
            SubState::Tx(index) => block.withdrawals().len() + index as usize,
            SubState::Block => return Ok(block.raw().post_account()),
        };

        let checkpoint = {
            let checkpoints = block.raw().state_checkpoint_list();
            checkpoints.get(checkpoint_idx).expect("checkpoint exists")
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

    fn extract_block_number_and_index_number(&self) -> (u64, u32) {
        match self.sub_state {
            SubState::Withdrawal(index) | SubState::Tx(index) => (self.block_number, index),
            SubState::Block => (self.block_number, 0),
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

    fn account_state_tree_with_merkle_state(
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
    // We should seperate the StateDB into ReadOnly & WriteOnly,
    // The ReadOnly is for fetching history state, and the write only is for writing new state.
    // This function should only be added on the ReadOnly state.
    pub fn account_smt(&self) -> Result<SMT<SMTStore<'_, Self>>, Error> {
        let merkle_state = self.get_checkpoint_merkle_state()?;
        self.account_smt_with_merkle_state(merkle_state)
    }

    pub fn account_state_tree(&self) -> Result<StateTree<'_, 'db>, Error> {
        let merkle_state = self.get_checkpoint_merkle_state()?;
        self.account_state_tree_with_merkle_state(merkle_state)
    }

    fn get_checkpoint_merkle_state(&self) -> Result<AccountMerkleState, Error> {
        let inner_db = self.inner;

        let account_merkle_state = match self.mode {
            StateDBMode::Genesis => {
                if 0 != self.checkpoint.block_number || SubState::Block != self.checkpoint.sub_state
                {
                    return Err(Error::from(format!(
                        "invalid check point {:?} for StateDBMode::Genesis",
                        self.checkpoint
                    )));
                }

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
            StateDBMode::Write => {
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
        let (block_number, tx_index) = self.checkpoint.extract_block_number_and_index_number();
        [key, &block_number.to_be_bytes(), &tx_index.to_be_bytes()].concat()
    }

    fn get_original_key<'a>(&self, raw_key: &'a [u8]) -> &'a [u8] {
        let (block_number, tx_index) = self.checkpoint.extract_block_number_and_index_number();
        &raw_key[..raw_key.len() - size_of_val(&block_number) - size_of_val(&tx_index)]
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

        let (block_number, tx_index) = self.checkpoint.extract_block_number_and_index_number();
        self.inner
            .record_block_state(block_number, tx_index, col, raw_key)?;
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

    /// submit tree changes into transaction
    /// notice, this function do not commit the DBTransaction
    pub fn submit_tree(&self) -> Result<(), Error> {
        self.db
            .inner
            .set_account_smt_root(*self.tree.root())
            .expect("set smt root");
        self.db
            .inner
            .set_account_count(self.account_count)
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
        match self.db.get(COLUMN_SCRIPT, script_hash.as_slice()) {
            Some(slice) => {
                Some(packed::ScriptReader::from_slice_should_be_ok(&slice.as_ref()).to_entity())
            }
            None => None,
        }
    }

    fn get_script_hash_by_prefix(&self, script_hash_prefix: &[u8]) -> Option<H256> {
        match self.db.get(COLUMN_SCRIPT_PREFIX, script_hash_prefix) {
            Some(slice) => {
                let mut hash = [0u8; 32];
                hash.copy_from_slice(&slice.as_ref());
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
        match self.db.get(COLUMN_DATA, data_hash.as_slice()) {
            Some(slice) => Some(Bytes::from(slice.to_vec())),
            None => None,
        }
    }
}
