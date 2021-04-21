//! State DB

use crate::{smt_store_impl::SMTStore, traits::KVStore, transaction::StoreTransaction};
use anyhow::{anyhow, Result};
use gw_common::{error::Error as StateError, smt::SMT, state::State, H256};
use gw_db::schema::{
    Col, COLUMN_ACCOUNT_SMT_BRANCH, COLUMN_ACCOUNT_SMT_LEAF, COLUMN_DATA, COLUMN_SCRIPT,
};
use gw_db::{error::Error, iter::DBIter, DBRawIterator, IteratorMode};
use gw_traits::CodeStore;
use gw_types::{
    bytes::Bytes,
    packed::{self, AccountMerkleState, TransactionKey},
    prelude::*,
};
use std::{cell::RefCell, collections::HashSet, fmt, mem::size_of_val};

const FLAG_DELETE_VALUE: u8 = 0;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StateDBVersion {
    Genesis,
    HistoryState { block_number: u64, tx_index: u32 },
    FutureState { block_number: u64, tx_index: u32 },
}

impl StateDBVersion {
    pub fn from_genesis() -> Self {
        StateDBVersion::Genesis
    }

    pub fn from_history_state(
        db: &StoreTransaction,
        block_hash: H256,
        tx_index: Option<u32>,
    ) -> Result<Self> {
        let block = db
            .get_block(&block_hash)?
            .ok_or_else(|| anyhow!("block isn't exist"))?;
        let block_number: u64 = block.raw().number().unpack();
        let tx_index =
            tx_index.unwrap_or_else(|| block.transactions().len().saturating_sub(1) as u32);
        if tx_index != 0 && tx_index as usize >= block.transactions().len() {
            return Err(anyhow!("Invalid tx index"));
        }
        Ok(StateDBVersion::HistoryState {
            block_number,
            tx_index,
        })
    }

    pub fn from_future_state(block_number: u64, tx_index: u32) -> Self {
        StateDBVersion::FutureState {
            block_number,
            tx_index,
        }
    }

    pub fn is_genesis_version(&self) -> bool {
        self == &Self::Genesis
    }

    fn extract_block_number_and_index_number(&self) -> (u64, u32) {
        match self {
            Self::Genesis => (0, 0),
            Self::HistoryState {
                block_number,
                tx_index,
            } => (*block_number, *tx_index),
            Self::FutureState {
                block_number,
                tx_index,
            } => (*block_number, *tx_index),
        }
    }
}

pub struct StateDBTransaction<'db> {
    inner: &'db StoreTransaction,
    version: StateDBVersion,
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
            .field("version", &self.version)
            .finish()
    }
}

impl<'db> StateDBTransaction<'db> {
    pub fn from_version(
        inner: &'db StoreTransaction,
        version: StateDBVersion,
    ) -> Result<Self, Error> {
        Ok(StateDBTransaction { inner, version })
    }

    pub fn commit(&self) -> Result<(), Error> {
        self.inner.commit()
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

    pub fn account_smt(&self) -> Result<SMT<SMTStore<'_, Self>>, Error> {
        let merkle_state = self.get_history_merkle_state()?;
        self.account_smt_with_merkle_state(merkle_state)
    }

    pub fn account_state_tree(&self) -> Result<StateTree<'_, 'db>, Error> {
        let merkle_state = self.get_history_merkle_state()?;
        self.account_state_tree_with_merkle_state(merkle_state)
    }

    fn get_history_merkle_state(&self) -> Result<AccountMerkleState, Error> {
        let account_merkle_state = match self.version {
            StateDBVersion::Genesis => match self.inner.get_block_hash_by_number(0)? {
                Some(block_hash) => {
                    let block = self
                        .inner
                        .get_block(&block_hash)?
                        .ok_or_else(|| "can't find genesis".to_string())?;
                    block.raw().post_account()
                }
                None => AccountMerkleState::default(),
            },
            StateDBVersion::HistoryState {
                block_number,
                tx_index,
            } => {
                let block_hash = self
                    .inner
                    .get_block_hash_by_number(block_number)?
                    .ok_or_else(|| "can't find block hash".to_string())?;
                let key = TransactionKey::build_transaction_key(block_hash.pack(), tx_index);
                match self.inner.get_transaction_receipt_by_key(&key)? {
                    Some(tx_receipt) => tx_receipt.post_state(),
                    None if tx_index == 0 => {
                        let block = self
                            .inner
                            .get_block(&block_hash)?
                            .ok_or_else(|| "can't find block".to_string())?;
                        block.raw().post_account()
                    }
                    None => panic!("inconsisted data"),
                }
            }
            StateDBVersion::FutureState {
                block_number,
                tx_index,
            } => {
                let mut last_block_number = block_number;
                let mut block_hash_opt = self.inner.get_block_hash_by_number(last_block_number)?;
                while block_hash_opt.is_none() {
                    last_block_number -= 1;
                    block_hash_opt = self.inner.get_block_hash_by_number(last_block_number)?;
                }
                let block_hash =
                    block_hash_opt.ok_or_else(|| "can't found block hash".to_string())?;
                let block = self
                    .inner
                    .get_block(&block_hash)?
                    .ok_or_else(|| "can't found block".to_string())?;
                let tx_index = if block.raw().number().unpack() == block_number {
                    // return tx_index if current block is future state block & tx exists
                    std::cmp::min(
                        block.transactions().len().saturating_sub(1) as u32,
                        tx_index,
                    )
                } else {
                    // otherwise return nearest block and tx_index
                    block.transactions().len().saturating_sub(1) as u32
                };

                let key = TransactionKey::build_transaction_key(block_hash.pack(), tx_index);
                match self.inner.get_transaction_receipt_by_key(&key)? {
                    Some(tx_receipt) => tx_receipt.post_state(),
                    None if tx_index == 0 => block.raw().post_account(),
                    None => panic!("inconsisted data"),
                }
            }
        };
        Ok(account_merkle_state)
    }

    fn get_key_with_suffix(&self, key: &[u8]) -> Vec<u8> {
        let (block_number, tx_index) = self.version.extract_block_number_and_index_number();
        [key, &block_number.to_be_bytes(), &tx_index.to_be_bytes()].concat()
    }

    fn get_original_key<'a>(&self, raw_key: &'a [u8]) -> &'a [u8] {
        let (block_number, tx_index) = self.version.extract_block_number_and_index_number();
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
        let (block_number, tx_index) = self.version.extract_block_number_and_index_number();
        // skip genesis
        if self.version.is_genesis_version() {
            return Ok(());
        }
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
    }

    fn get_script(&self, script_hash: &H256) -> Option<packed::Script> {
        match self.db.get(COLUMN_SCRIPT, script_hash.as_slice()) {
            Some(slice) => {
                Some(packed::ScriptReader::from_slice_should_be_ok(&slice.as_ref()).to_entity())
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
