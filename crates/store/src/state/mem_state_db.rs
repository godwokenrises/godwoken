//! State DB

use std::collections::HashMap;

use crate::smt::mem_smt_store::MemSMTStore;
use crate::{traits::KVStore, transaction::StoreTransaction};
use anyhow::Result;
use gw_common::{error::Error as StateError, smt::SMT, state::State, H256};
use gw_db::schema::{COLUMN_DATA, COLUMN_SCRIPT, COLUMN_SCRIPT_PREFIX};
use gw_traits::CodeStore;
use gw_types::{
    bytes::Bytes,
    packed::{self, AccountMerkleState},
    prelude::*,
};

use super::state_tracker::{self, StateTracker};

/// MemStateTree
/// This struct is used for calculate state in the memory
pub struct MemStateTree<'a> {
    tree: SMT<MemSMTStore<'a>>,
    account_count: u32,
    tracker: StateTracker,
    scripts: HashMap<H256, packed::Script>,
    data: HashMap<H256, Bytes>,
    scripts_hash_prefix: HashMap<Bytes, H256>,
}

impl<'a> MemStateTree<'a> {
    pub fn new(tree: SMT<MemSMTStore<'a>>, account_count: u32) -> Self {
        MemStateTree {
            tree,
            account_count,
            scripts: Default::default(),
            data: Default::default(),
            scripts_hash_prefix: Default::default(),
            tracker: StateTracker::default(),
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

    fn db(&self) -> &StoreTransaction {
        &self.tree.store().inner_store()
    }
}

impl<'a> State for MemStateTree<'a> {
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

impl<'a> CodeStore for MemStateTree<'a> {
    fn insert_script(&mut self, script_hash: H256, script: packed::Script) {
        self.scripts.insert(script_hash, script);
        // build script_hash prefix search index
        self.scripts_hash_prefix
            .insert(script_hash.as_slice()[..20].to_vec().into(), script_hash);
    }

    fn get_script(&self, script_hash: &H256) -> Option<packed::Script> {
        self.scripts.get(script_hash).cloned().or_else(|| {
            self.db()
                .get(COLUMN_SCRIPT, script_hash.as_slice())
                .map(|slice| {
                    packed::ScriptReader::from_slice_should_be_ok(slice.as_ref()).to_entity()
                })
        })
    }

    fn get_script_hash_by_short_address(&self, script_hash_prefix: &[u8]) -> Option<H256> {
        self.scripts_hash_prefix
            .get(&Bytes::from(script_hash_prefix.to_vec()))
            .cloned()
            .or_else(
                || match self.db().get(COLUMN_SCRIPT_PREFIX, script_hash_prefix) {
                    Some(slice) => {
                        let mut hash = [0u8; 32];
                        hash.copy_from_slice(slice.as_ref());
                        Some(hash.into())
                    }
                    None => None,
                },
            )
    }

    fn insert_data(&mut self, data_hash: H256, code: Bytes) {
        self.data.insert(data_hash, code);
    }

    fn get_data(&self, data_hash: &H256) -> Option<Bytes> {
        self.data.get(data_hash).cloned().or_else(|| {
            self.db()
                .get(COLUMN_DATA, data_hash.as_slice())
                .map(|slice| Bytes::from(slice.to_vec()))
        })
    }
}
