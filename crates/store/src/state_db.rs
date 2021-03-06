//! State DB

use std::{cell::RefCell, collections::HashSet};

use crate::{smt_store_impl::SMTStore, traits::KVStore, transaction::StoreTransaction};
use gw_common::{error::Error as StateError, smt::SMT, state::State, H256};
use gw_db::schema::{
    Col, COLUMN_ACCOUNT_SMT_BRANCH, COLUMN_ACCOUNT_SMT_LEAF, COLUMN_DATA, COLUMN_SCRIPT,
};
use gw_db::{error::Error, iter::DBIter, DBVector, IteratorMode};
use gw_traits::CodeStore;
use gw_types::{bytes::Bytes, packed, prelude::*};

pub struct StateDBVersion(H256);

impl StateDBVersion {
    /// TODO implement this
    pub fn from_block_hash(block_hash: H256) -> Self {
        StateDBVersion(block_hash)
    }

    /// TODO implement this
    pub fn from_genesis() -> Self {
        Self(Default::default())
    }

    pub fn get_block_hash(&self) -> H256 {
        self.0
    }
}

// TODO implement this
pub struct StateDBTransaction {
    inner: StoreTransaction,
    block_number: u64,
    tx_index: u32,
}

impl KVStore for StateDBTransaction {
    fn get(&self, col: Col, key: &[u8]) -> Option<DBVector> {
        self.inner.get(col, key)
    }

    fn get_iter(&self, col: Col, mode: IteratorMode) -> DBIter {
        self.inner.get_iter(col, mode)
    }

    fn insert_raw(&self, col: Col, key: &[u8], value: &[u8]) -> Result<(), Error> {
        self.inner.insert_raw(col, key, value)
    }

    fn delete(&self, col: Col, key: &[u8]) -> Result<(), Error> {
        self.inner.delete(col, key)
    }
}

impl StateDBTransaction {
    pub fn from_version(inner: StoreTransaction, version: StateDBVersion) -> Self {
        let (block_number, tx_index) = StateDBTransaction::get_tx_index(&inner, version);
        StateDBTransaction { inner, block_number, tx_index }
    }

    fn get_tx_index(_inner: &StoreTransaction, version: StateDBVersion) -> (u64, u32) {
        let _block_hash = version.0;
        unimplemented!();
    }

    // TODO: set private so can be used by unit tests 
    pub fn from_tx_index(inner: StoreTransaction, block_number: u64, tx_index: u32) -> Self {
        StateDBTransaction { inner, block_number, tx_index }
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
