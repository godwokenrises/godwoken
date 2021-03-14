//! State DB

use std::{cell::RefCell, collections::HashSet, mem::size_of_val};

use crate::{smt_store_impl::SMTStore, traits::KVStore, transaction::StoreTransaction};
use gw_common::{error::Error as StateError, smt::SMT, state::State, H256};
use gw_db::schema::{
    Col, COLUMN_ACCOUNT_SMT_BRANCH, COLUMN_ACCOUNT_SMT_LEAF, COLUMN_DATA, COLUMN_SCRIPT,
};
use gw_db::{error::Error, iter::DBIter, IteratorMode, DBRawIterator};
use gw_traits::CodeStore;
use gw_types::{bytes::Bytes, packed, prelude::*};

// StateDBTransaction's delete method insert the value 0u8 presents the key to be deleted.
const DELETE_FLAG_VALUE: u8 = 0;

// Error message 
const ERROR_MSG_STATE_DB_INIT_BLOCK_NUM_NOT_EXIST: &str = "Block number do not exists when state db transaction init";
const ERROR_MSG_STATE_DB_INIT_TX_INDEX_NOT_EXIST: &str = "Tx number do not exists when state db transaction init";

pub struct StateDBVersion {
    block_hash: H256,
    tx_index: Option<u32>,
}

impl StateDBVersion {
    pub fn from_genesis() -> Self {
        StateDBVersion { 
            block_hash: H256::zero(), 
            tx_index: None,
        }
    }

    pub fn from_block_hash(block_hash: H256) -> Self {
        StateDBVersion { 
            block_hash, 
            tx_index: None, 
        }
    }

    pub fn from_tx_index(block_hash: H256, tx_index: u32) -> Self {
        StateDBVersion { 
            block_hash, 
            tx_index: Some(tx_index),
        }
    }
}

pub struct StateDBTransaction {
    inner: StoreTransaction,
    block_num: u64,
    tx_index: u32,
}

impl KVStore for StateDBTransaction {
    fn get(&self, col: Col, key: &[u8]) -> Option<Box<[u8]>> {
        let raw_key = self.get_key_with_ver(key);
        let mut raw_iter: DBRawIterator = self.inner.get_iter(col, IteratorMode::Start).into();
        raw_iter.seek_for_prev(raw_key);
        self.filter_value_after_seek(key, &raw_iter)
    }

    fn get_iter(&self, col: Col, mode: IteratorMode) -> DBIter {
        self.inner.get_iter(col, mode)
    }

    fn insert_raw(&self, col: Col, key: &[u8], value: &[u8]) -> Result<(), Error> {
        let raw_key = self.get_key_with_ver(key);
        self.inner.insert_raw(col, &raw_key, value)
    }
 
    fn delete(&self, col: Col, key: &[u8]) -> Result<(), Error> {
        let raw_key = self.get_key_with_ver(key);
        self.inner.insert_raw(col, &raw_key, &DELETE_FLAG_VALUE.to_be_bytes())
    }
}

impl StateDBTransaction {
    pub fn from_version(inner: StoreTransaction, ver: StateDBVersion) -> Result<Self, Error> {
        if StateDBTransaction::is_genesis_block(&ver) { 
            return Ok(StateDBTransaction::from_tx_index(inner, 0u64, 0u32)); 
        }

        let block_num = StateDBTransaction::get_block_num(&inner, &ver.block_hash)?;
        let block_num = block_num.ok_or_else(|| ERROR_MSG_STATE_DB_INIT_BLOCK_NUM_NOT_EXIST.to_string())?;
        let tx_idx = StateDBTransaction::get_tx_index(&inner, block_num)?;
        let tx_idx = tx_idx.ok_or_else(|| ERROR_MSG_STATE_DB_INIT_TX_INDEX_NOT_EXIST.to_string())?;

        Ok(StateDBTransaction::from_tx_index(inner, block_num, tx_idx))
    }

    fn is_genesis_block(ver: &StateDBVersion) -> bool {
        ver.block_hash == H256::zero() && ver.tx_index.is_none()
    }

    fn get_block_num(inner: &StoreTransaction, block_hash: &H256) -> Result<Option<u64>, Error> {
        inner.get_block_number(block_hash)
    }

    fn get_tx_index(_inner: &StoreTransaction, _block_num: u64) -> Result<Option<u32>, Error> {
        unimplemented!()
    }

    // this private constructor can be injected with mock data by unit test
    fn from_tx_index(inner: StoreTransaction, block_num: u64, tx_index: u32) -> Self {
        StateDBTransaction { inner, block_num, tx_index }
    }

    pub fn commit(&self) -> Result<(), Error> {
        self.inner.commit()
    }

    pub fn account_smt_store<'a>(&'a self) -> Result<SMTStore<'a, Self>, Error> {
        let smt_store = SMTStore::new(COLUMN_ACCOUNT_SMT_LEAF, COLUMN_ACCOUNT_SMT_BRANCH, self);
        Ok(smt_store)
    }

    pub fn account_smt<'a>(&'a self) -> Result<SMT<SMTStore<'a, Self>>, Error> {
        let root = self.inner.get_account_smt_root()?;
        let smt_store = self.account_smt_store()?;
        Ok(SMT::new(root, smt_store))
    }

    pub fn account_state_tree<'a>(&'a self) -> Result<StateTree<'a>, Error> {
        let account_count = self.inner.get_account_count()?;
        Ok(StateTree::new(self, self.account_smt()?, account_count))
    }

    /// TODO refacotring with version based DB
    /// clear account state tree, delete leaves and branches from DB
    pub fn clear_account_state_tree(&self) -> Result<(), Error> {
        self.inner.set_account_smt_root(H256::zero())?;
        self.inner.set_account_count(0)?;
        for col in &[COLUMN_ACCOUNT_SMT_LEAF, COLUMN_ACCOUNT_SMT_BRANCH] {
            for (k, _v) in self.get_iter(col, IteratorMode::Start) {
                self.delete(col, k.as_ref())?;
            }
        }
        Ok(())
    }

    fn get_key_with_ver(&self, key: &[u8]) -> Vec<u8> {
        [key, &self.block_num.to_be_bytes(), &self.tx_index.to_be_bytes()].concat()
    }

    fn get_ori_key<'a>(&self, key: &'a [u8]) -> &'a[u8] {
        &key[..key.len()-size_of_val(&self.block_num)-size_of_val(&self.tx_index)]
    }

    fn filter_value_after_seek(&self, ori_key: &[u8], raw_iter: &DBRawIterator) -> Option<Box<[u8]>> {
        if !raw_iter.valid() { return None; }
        match raw_iter.key() {
            Some(raw_key_found) => {
                if ori_key != self.get_ori_key(raw_key_found) {
                    return None;
                } 
                match raw_iter.value() {
                    Some(&[DELETE_FLAG_VALUE]) => None,
                    Some(value) => Some(Box::<[u8]>::from(value)),
                    None => None,
                }
            },
            None => None,
        }
    }
}

/// Tracker state changes
pub struct StateTracker {
    touched_keys: Option<RefCell<HashSet<H256>>>,
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

pub struct StateTree<'a> {
    tree: SMT<SMTStore<'a, StateDBTransaction>>,
    account_count: u32,
    db: &'a StateDBTransaction,
    tracker: StateTracker,
}

impl<'a> StateTree<'a> {
    pub fn new(
        db: &'a StateDBTransaction,
        tree: SMT<SMTStore<'a, StateDBTransaction>>,
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

impl<'a> State for StateTree<'a> {
    fn get_raw(&self, key: &H256) -> Result<H256, StateError> {
        self.tracker.touch_key(key);
        let v = self.tree.get(&(*key).into())?;
        Ok(v.into())
    }
    fn update_raw(&mut self, key: H256, value: H256) -> Result<(), StateError> {
        self.tracker.touch_key(&key);
        self.tree.update(key.into(), value.into())?;
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

impl<'a> CodeStore for StateTree<'a> {
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

#[cfg(test)]
mod tests;
