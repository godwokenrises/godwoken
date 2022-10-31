//! StateDB
//!
//! StateDB is build in layers, for example:
//!
//! - StateDB (maintain in-memory journal and snapshot)
//! - HistoryState (persist histories block state)
//! - FileDB (RocksDB)
//!

use std::{collections::HashSet, sync::Mutex};

use anyhow::{anyhow, Result};
use gw_traits::CodeStore;
use gw_types::{
    bytes::Bytes,
    packed,
    prelude::{Pack, Unpack},
};

use gw_common::{error::Error as StateError, smt::SMT, state::State, H256};

use crate::{
    smt::smt_store::SMTStateStore,
    snapshot::StoreSnapshot,
    state::history::history_state::{HistoryState, HistoryStateStore},
    traits::{chain_store::ChainStore, kv_store::KVStore},
};

use super::{
    history::history_state::RWConfig,
    overlay::{mem_state::MemStateTree, mem_store::MemStore},
    BlockStateDB, MemStateDB,
};

#[derive(Debug, Clone)]
pub enum JournalEntry {
    UpdateRaw { key: H256, prev_value: H256 },
    SetAccountCount { prev_count: u32 },
    InsertScript { script_hash: H256, prev_exist: bool },
    InsertData { data_hash: H256, prev_exist: bool },
}

impl JournalEntry {
    /// revert journal
    pub fn revert<S>(self, state_db: &mut StateDB<S>) {
        use JournalEntry::*;

        match self {
            UpdateRaw { key, prev_value } => {
                state_db.dirty_state.insert(key, prev_value);
            }
            SetAccountCount { prev_count } => {
                state_db.dirty_account_count = Some(prev_count);
            }
            InsertScript {
                script_hash,
                prev_exist,
            } => {
                if !prev_exist {
                    state_db.dirty_scripts.remove(&script_hash);
                }
            }
            InsertData {
                data_hash,
                prev_exist,
            } => {
                if !prev_exist {
                    state_db.dirty_data.remove(&data_hash);
                }
            }
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct Journal {
    entries: im::Vector<JournalEntry>,
}

impl Journal {
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Revert journal to len
    /// Return reverted entries sorted in journal emit order
    pub fn revert_entries(&mut self, len: usize) -> im::Vector<JournalEntry> {
        self.entries.slice(len..)
    }

    pub fn push(&mut self, entry: JournalEntry) {
        self.entries.push_back(entry);
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

#[derive(Debug, Clone)]
pub struct Revision {
    id: usize,
    journal_len: usize,
}

#[derive(Debug, Default)]
pub struct StateTracker {
    touched_keys: Mutex<HashSet<H256>>,
}

impl StateTracker {
    /// Return touched keys
    pub fn touched_keys(&self) -> &Mutex<HashSet<H256>> {
        &self.touched_keys
    }

    /// Record a key in the tracker
    pub fn touch_key(&self, key: &H256) {
        self.touched_keys.lock().unwrap().insert(*key);
    }

    /// Record a key in the tracker
    pub fn clear(&mut self) {
        self.touched_keys.lock().unwrap().clear();
    }
}

pub struct StateDB<S> {
    /// inner state
    state: S,
    /// journals
    journal: Journal,
    /// next_revision_id
    next_revision_id: usize,
    /// revisions
    revisions: im::Vector<Revision>,
    /// dirty state in memory
    dirty_state: im::HashMap<H256, H256>,
    /// dirty account count
    dirty_account_count: Option<u32>,
    /// dirty scripts
    dirty_scripts: im::HashMap<H256, packed::Script>,
    /// dirty data
    dirty_data: im::HashMap<H256, Bytes>,
    /// state tracker
    state_tracker: Option<StateTracker>,
    /// last state root
    last_state_root: H256,
}

impl<S: Clone> Clone for StateDB<S> {
    fn clone(&self) -> Self {
        Self {
            state: self.state.clone(),
            journal: self.journal.clone(),
            next_revision_id: self.next_revision_id,
            revisions: self.revisions.clone(),
            dirty_state: self.dirty_state.clone(),
            dirty_account_count: self.dirty_account_count,
            dirty_scripts: self.dirty_scripts.clone(),
            dirty_data: self.dirty_data.clone(),
            state_tracker: None,
            last_state_root: self.last_state_root,
        }
    }
}

impl MemStateDB {
    /// From store
    pub fn from_store(store: StoreSnapshot) -> Result<Self> {
        // build from last valid block
        let block = store.get_last_valid_tip_block()?;
        let tip_state = block.raw().post_account();
        let smt = SMT::new(
            tip_state.merkle_root().unpack(),
            SMTStateStore::new(MemStore::new(store)),
        );
        let inner = MemStateTree::new(smt, tip_state.count().unpack());
        Ok(Self::new(inner))
    }

    pub fn inner_smt_tree(&self) -> &SMT<SMTStateStore<MemStore>> {
        self.state.smt()
    }
}

impl<Store: ChainStore + HistoryStateStore + CodeStore + KVStore> BlockStateDB<Store> {
    /// From store
    pub fn from_store(store: Store, rw_config: RWConfig) -> Result<Self> {
        // build from last valid block
        let block = store.get_last_valid_tip_block()?;
        let tip_state = block.raw().post_account();
        let smt = SMT::new(tip_state.merkle_root().unpack(), SMTStateStore::new(store));
        let inner = HistoryState::new(smt, tip_state.count().unpack(), rw_config);
        Ok(Self::new(inner))
    }

    /// Detach block state
    /// The caller must avoid has dirty state, otherwise, the state may inconsisted after the detaching
    pub fn detach_block_state(&mut self, block_number: u64) -> Result<()> {
        self.state.detach_block_state(block_number)
    }
}

impl<S: State + CodeStore> StateDB<S> {
    pub fn new(mut state: S) -> Self {
        let last_state_root = state.finalise_root().expect("can't get state root");
        Self {
            state,
            journal: Journal::default(),
            next_revision_id: 0,
            revisions: Default::default(),
            dirty_state: Default::default(),
            dirty_account_count: Default::default(),
            dirty_data: Default::default(),
            dirty_scripts: Default::default(),
            state_tracker: None,
            last_state_root,
        }
    }

    // Perform basic state checking
    pub fn check_state(&self) -> Result<()> {
        let non_exit_account = self.get_script_hash(self.get_account_count()?)?;
        assert_eq!(
            non_exit_account,
            H256::zero(),
            "Detect inconsistent state: account {} should be non-exist",
            self.get_account_count()?
        );

        // check first 100 account
        for i in 0..std::cmp::min(100, self.get_account_count()?) {
            let script_hash = self.get_script_hash(i)?;
            assert_ne!(
                script_hash,
                H256::zero(),
                "Detect inconsistent state: account {} should exist",
                i
            );
            assert!(
                self.get_script(&script_hash).is_some(),
                "Detect inconsistent state: script {} not exist",
                {
                    let hash: packed::Byte32 = script_hash.pack();
                    hash
                }
            );
        }

        // check last 100 account
        for i in self.get_account_count()?.saturating_sub(100)..self.get_account_count()? {
            let script_hash = self.get_script_hash(i)?;
            assert_ne!(
                script_hash,
                H256::zero(),
                "Detect inconsistent state: account {} should exist",
                i
            );
            assert!(
                self.get_script(&script_hash).is_some(),
                "Detect inconsistent state: script {} not exist",
                {
                    let hash: packed::Byte32 = script_hash.pack();
                    hash
                }
            );
        }

        Ok(())
    }

    /// Return last state root
    /// Invoke `calculate_root` will update this value
    pub fn last_state_root(&self) -> H256 {
        self.last_state_root
    }

    /// create snapshot
    pub fn snapshot(&mut self) -> usize {
        let id = self.next_revision_id;
        self.next_revision_id += 1;
        self.revisions.push_back(Revision {
            id,
            journal_len: self.journal.len(),
        });
        id
    }

    /// revert to a snapshot
    pub fn revert(&mut self, id: usize) -> Result<()> {
        // find revision
        let rev_index = self
            .revisions
            .binary_search_by_key(&id, |r| r.id)
            .map_err(|_id| anyhow!("Invalid revision id"))?;
        let rev = &self.revisions[rev_index];

        // replay to revert journal
        let revert_entries = self.journal.revert_entries(rev.journal_len);
        for entry in revert_entries.into_iter().rev() {
            entry.revert(self);
        }

        // remove expired revisions
        self.revisions.truncate(rev_index);
        Ok(())
    }

    /// write dirty state to DB
    pub fn finalise(&mut self) -> Result<()> {
        // write account count
        if let Some(count) = self.dirty_account_count {
            self.state.set_account_count(count)?;
        }
        // write state
        for (k, v) in &self.dirty_state {
            self.state.update_raw(*k, *v)?;
        }
        // write scripts
        for (script_hash, script) in &self.dirty_scripts {
            self.state.insert_script(*script_hash, script.to_owned());
        }
        // write data
        for (data_hash, data) in &self.dirty_data {
            self.state.insert_data(*data_hash, data.to_owned());
        }
        // clear
        self.clear_journal_and_dirty()
    }

    #[cfg(test)]
    pub(crate) fn is_dirty(&self) -> bool {
        self.journal.len() != 0
            || !self.revisions.is_empty()
            || self.dirty_account_count.is_some()
            || !self.dirty_state.is_empty()
            || !self.dirty_scripts.is_empty()
            || !self.dirty_data.is_empty()
    }

    /// clear journal and dirty
    fn clear_journal_and_dirty(&mut self) -> Result<()> {
        // clear journal
        self.journal.clear();
        // clear revisions
        self.revisions.clear();
        // clear dirties
        self.dirty_account_count = None;
        self.dirty_state.clear();
        self.dirty_scripts.clear();
        self.dirty_data.clear();
        Ok(())
    }

    pub fn set_state_tracker(&mut self, tracker: StateTracker) {
        self.state_tracker = Some(tracker);
    }

    pub fn state_tracker(&self) -> Option<&StateTracker> {
        self.state_tracker.as_ref()
    }

    pub fn take_state_tracker(&mut self) -> Option<StateTracker> {
        self.state_tracker.take()
    }
}

impl<S: State + CodeStore> State for StateDB<S> {
    fn get_raw(&self, key: &H256) -> Result<H256, StateError> {
        if let Some(tracker) = self.state_tracker.as_ref() {
            tracker.touch_key(key);
        }
        if let Some(v) = self.dirty_state.get(key) {
            return Ok(*v);
        }
        self.state.get_raw(key)
    }

    fn update_raw(&mut self, key: H256, value: H256) -> Result<(), StateError> {
        if let Some(tracker) = self.state_tracker.as_ref() {
            tracker.touch_key(&key);
        }
        self.journal.push(JournalEntry::UpdateRaw {
            key,
            prev_value: self.get_raw(&key)?,
        });
        self.dirty_state.insert(key, value);
        Ok(())
    }

    fn get_account_count(&self) -> Result<u32, StateError> {
        if let Some(count) = self.dirty_account_count {
            return Ok(count);
        }
        self.state.get_account_count()
    }

    fn set_account_count(&mut self, count: u32) -> Result<(), StateError> {
        self.journal.push(JournalEntry::SetAccountCount {
            prev_count: self.get_account_count()?,
        });
        self.dirty_account_count = Some(count);
        Ok(())
    }

    /// Finalise in-memory dirty state and calculate root
    fn finalise_root(&mut self) -> Result<H256, StateError> {
        // finalise dirty state
        self.finalise().map_err(|err| {
            log::error!("finalise error: {}", err);
            StateError::Store
        })?;
        self.last_state_root = self.state.finalise_root()?;
        Ok(self.last_state_root)
    }
}

impl<S: CodeStore> CodeStore for StateDB<S> {
    fn insert_script(&mut self, script_hash: H256, script: packed::Script) {
        self.journal.push(JournalEntry::InsertScript {
            script_hash,
            prev_exist: self.get_script(&script_hash).is_some(),
        });
        self.dirty_scripts.insert(script_hash, script);
    }

    fn get_script(&self, script_hash: &H256) -> Option<packed::Script> {
        if let Some(script) = self.dirty_scripts.get(script_hash) {
            return Some(script.clone());
        }
        self.state.get_script(script_hash)
    }

    fn insert_data(&mut self, data_hash: H256, code: Bytes) {
        self.journal.push(JournalEntry::InsertData {
            data_hash,
            prev_exist: self.get_data(&data_hash).is_some(),
        });
        self.dirty_data.insert(data_hash, code);
    }

    fn get_data(&self, data_hash: &H256) -> Option<Bytes> {
        if let Some(data) = self.dirty_data.get(data_hash) {
            return Some(data.to_owned());
        }
        self.state.get_data(data_hash)
    }
}

#[cfg(test)]
mod tests {
    use gw_common::{h256_ext::H256Ext, smt::SMT, state::State, H256};
    use gw_traits::CodeStore;

    use crate::{
        smt::smt_store::SMTStateStore,
        snapshot::StoreSnapshot,
        state::{
            overlay::{mem_state::MemStateTree, mem_store::MemStore},
            MemStateDB,
        },
        Store,
    };

    fn new_state(store: StoreSnapshot) -> MemStateDB {
        let smt = SMT::new(H256::zero(), SMTStateStore::new(MemStore::new(store)));
        let inner = MemStateTree::new(smt, 0);
        MemStateDB::new(inner)
    }

    fn cmp_dirty_state(state_a: &MemStateDB, state_b: &MemStateDB) -> bool {
        // account count
        if state_a.get_account_count() != state_b.get_account_count() {
            return false;
        }
        // scripts
        for script_hash in state_a
            .dirty_scripts
            .keys()
            .chain(state_b.dirty_scripts.keys())
        {
            if state_a.get_script(script_hash) != state_b.get_script(script_hash) {
                return false;
            }
        }
        // data
        for data_hash in state_a.dirty_data.keys().chain(state_b.dirty_data.keys()) {
            if state_a.get_data(data_hash) != state_b.get_data(data_hash) {
                return false;
            }
        }
        // kv
        for key in state_a.dirty_state.keys().chain(state_b.dirty_state.keys()) {
            if state_a.get_raw(key) != state_b.get_raw(key) {
                return false;
            }
        }

        true
    }

    #[test]
    fn test_revert_to_histories_revision() {
        let store = Store::open_tmp().unwrap();
        let mut state = new_state(store.get_snapshot());
        let mem_0 = state.clone();
        let snap_0 = state.snapshot();
        state
            .update_raw(H256::from_u32(1), H256::from_u32(1))
            .unwrap();
        state
            .update_raw(H256::from_u32(2), H256::from_u32(2))
            .unwrap();
        let mem_1 = state.clone();
        let snap_1 = state.snapshot();

        // should update the mem DB
        state
            .update_raw(H256::from_u32(3), H256::from_u32(3))
            .unwrap();
        let mem_2 = state.clone();
        assert!(!cmp_dirty_state(&mem_1, &mem_2));

        // revert to snap_1
        state.revert(snap_1).unwrap();
        assert!(cmp_dirty_state(&mem_1, &state));

        // revert to snap_0
        state.revert(snap_0).unwrap();
        assert!(cmp_dirty_state(&mem_0, &state));
    }

    #[test]
    fn test_clear_dirty_state() {
        let store = Store::open_tmp().unwrap();
        let mut state = new_state(store.get_snapshot());
        let mem_0 = state.clone();
        assert!(!state.is_dirty());
        state
            .update_raw(H256::from_u32(1), H256::from_u32(1))
            .unwrap();
        state
            .update_raw(H256::from_u32(2), H256::from_u32(2))
            .unwrap();
        assert!(state.is_dirty());

        // clear journal and dirty state
        state.clear_journal_and_dirty().unwrap();
        assert!(!state.is_dirty());
        // the dirty state is cleared
        assert!(cmp_dirty_state(&mem_0, &state));
    }

    #[test]
    fn test_finalise_dirty_state() {
        let store = Store::open_tmp().unwrap();
        let mut state = new_state(store.get_snapshot());
        assert!(!state.is_dirty());
        state
            .update_raw(H256::from_u32(1), H256::from_u32(1))
            .unwrap();
        state
            .update_raw(H256::from_u32(2), H256::from_u32(2))
            .unwrap();
        let mem_1 = state.clone();
        assert!(state.is_dirty());

        // finalise
        state.finalise().unwrap();
        assert!(!state.is_dirty());
        // the dirty state is cleared, but the state is write into the store
        assert!(cmp_dirty_state(&mem_1, &state));
    }
}
