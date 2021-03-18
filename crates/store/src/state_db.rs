//! State DB

use std::{cell::RefCell, collections::HashSet, mem::size_of_val};

use crate::{smt_store_impl::SMTStore, traits::KVStore, transaction::StoreTransaction};
use gw_common::{error::Error as StateError, smt::SMT, state::State, H256};
use gw_db::schema::{
    Col, COLUMN_ACCOUNT_SMT_BRANCH, COLUMN_ACCOUNT_SMT_LEAF, COLUMN_DATA, COLUMN_SCRIPT,
};
use gw_db::{error::Error, iter::DBIter, DBRawIterator, IteratorMode};
use gw_traits::CodeStore;
use gw_types::{bytes::Bytes, packed, prelude::*};

const FLAG_DELETE_VALUE: u8 = 0;
const BLOCK_NUMBER_ZERO: u64 = 0;
const TX_INDEX_ZERO: u32 = 0;

pub struct StateDBVersion {
    block_hash: Option<H256>,
    tx_index: Option<u32>,
}

impl StateDBVersion {
    pub fn from_genesis() -> Self {
        StateDBVersion {
            block_hash: None,
            tx_index: None,
        }
    }

    pub fn from_block_hash(block_hash: H256) -> Self {
        StateDBVersion {
            block_hash: Some(block_hash),
            tx_index: None,
        }
    }

    pub fn from_tx_index(block_hash: H256, tx_index: u32) -> Self {
        StateDBVersion {
            block_hash: Some(block_hash),
            tx_index: Some(tx_index),
        }
    }

    pub fn is_genesis_version(&self) -> bool {
        self.block_hash.is_none() && self.tx_index.is_none()
    }
}

pub struct StateDBTransaction {
    inner: StoreTransaction,
    block_num: u64,
    tx_index: u32,
}

impl KVStore for StateDBTransaction {
    fn get(&self, col: Col, key: &[u8]) -> Option<Box<[u8]>> {
        let raw_key = self.get_key_with_ver_sfx(key);
        let mut raw_iter: DBRawIterator = self.inner.get_iter(col, IteratorMode::Start).into();
        raw_iter.seek_for_prev(raw_key);
        self.filter_value_of_seek(key, &raw_iter)
    }

    // TODO: this trait method will be deleted in the future.
    fn get_iter(&self, col: Col, mode: IteratorMode) -> DBIter {
        self.inner.get_iter(col, mode)
    }

    fn insert_raw(&self, col: Col, key: &[u8], value: &[u8]) -> Result<(), Error> {
        let raw_key = self.get_key_with_ver_sfx(key);
        self.inner.insert_raw(col, &raw_key, value)
    }

    fn delete(&self, col: Col, key: &[u8]) -> Result<(), Error> {
        let raw_key = self.get_key_with_ver_sfx(key);
        self.inner
            .insert_raw(col, &raw_key, &FLAG_DELETE_VALUE.to_be_bytes())
    }
}

impl StateDBTransaction {
    pub fn from_version(inner: StoreTransaction, ver: StateDBVersion) -> Result<Self, Error> {
        let (block_num, tx_idx) = StateDBTransaction::get_block_num_and_tx_index(&inner, &ver)?;
        Ok(StateDBTransaction::from_tx_index(inner, block_num, tx_idx))
    }

    pub fn commit(&self) -> Result<(), Error> {
        self.inner.commit()
    }

    pub fn account_smt_store<'a>(&'a self) -> Result<SMTStore<'a, Self>, Error> {
        let smt_store = SMTStore::new(COLUMN_ACCOUNT_SMT_LEAF, COLUMN_ACCOUNT_SMT_BRANCH, self);
        Ok(smt_store)
    }

    pub fn account_smt<'a>(&'a self) -> Result<SMT<SMTStore<'a, Self>>, Error> {
        // let root = self.inner.get_account_smt_root()?;
        let (root, _) = self.get_account_smt_root_and_count()?;
        let smt_store = self.account_smt_store()?;
        Ok(SMT::new(root, smt_store))
    }

    pub fn account_state_tree<'a>(&'a self) -> Result<StateTree<'a>, Error> {
        // let account_count = self.inner.get_account_count()?;
        let (_, account_count) = self.get_account_smt_root_and_count()?;
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

    fn get_block_num_and_tx_index(
        inner: &StoreTransaction,
        ver: &StateDBVersion,
    ) -> Result<(u64, u32), Error> {
        if ver.is_genesis_version() {
            return Ok((BLOCK_NUMBER_ZERO, TX_INDEX_ZERO));
        }
        let block_hash = &ver
            .block_hash
            .ok_or_else(|| "Block hash doesn't exist".to_owned())?;
        let block_num = inner
            .get_block_number(block_hash)?
            .ok_or_else(|| "Block num doesn't exist".to_owned())?;
        let tx_idx = inner
            .get_block(block_hash)
            .map(|blk| {
                if let Some(block) = blk {
                    let txs = block.transactions();
                    if let Some(tx_idx) = ver.tx_index {
                        if txs.get(tx_idx as usize).is_some() {
                            Some(tx_idx)
                        } else {
                            None
                        }
                    } else {
                        Some(txs.item_count() as u32)
                    }
                } else {
                    None
                }
            })?
            .ok_or_else(|| "Tx index doesn't exist".to_owned())?;
        Ok((block_num, tx_idx))
    }

    // This private constructor can be injected with mock data by unit test
    fn from_tx_index(inner: StoreTransaction, block_num: u64, tx_index: u32) -> Self {
        StateDBTransaction {
            inner,
            block_num,
            tx_index,
        }
    }

    fn get_account_smt_root_and_count(&self) -> Result<(H256, u32), Error> {
        if self.block_num == 0 && self.tx_index == 0 {
            return Ok((H256::zero(), 0));
        } // TODO: refactoring
        let block_hash = self
            .inner
            .get_block_hash_by_number(self.block_num)?
            .ok_or_else(|| "Block hash doesn't exist".to_owned())?;
        let block = self
            .inner
            .get_block(&block_hash)?
            .ok_or_else(|| "Block doesnt exist".to_owned())?;
        let account_merkle_state = block.raw().post_account();
        Ok((
            account_merkle_state.merkle_root().unpack(),
            account_merkle_state.count().unpack(),
        ))
    }

    fn get_key_with_ver_sfx(&self, key: &[u8]) -> Vec<u8> {
        [
            key,
            &self.block_num.to_be_bytes(),
            &self.tx_index.to_be_bytes(),
        ]
        .concat()
    }

    fn get_ori_key<'a>(&self, raw_key: &'a [u8]) -> &'a [u8] {
        &raw_key[..raw_key.len() - size_of_val(&self.block_num) - size_of_val(&self.tx_index)]
    }

    fn filter_value_of_seek(&self, ori_key: &[u8], raw_iter: &DBRawIterator) -> Option<Box<[u8]>> {
        if !raw_iter.valid() {
            return None;
        }
        match raw_iter.key() {
            Some(raw_key_found) => {
                if ori_key != self.get_ori_key(raw_key_found) {
                    return None;
                }
                match raw_iter.value() {
                    Some(&[FLAG_DELETE_VALUE]) => None,
                    Some(value) => Some(Box::<[u8]>::from(value)),
                    None => None,
                }
            }
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
