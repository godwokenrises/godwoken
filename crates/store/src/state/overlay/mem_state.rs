//! Mem State DB
//!

use crate::schema::{COLUMN_DATA, COLUMN_SCRIPT};
use crate::smt::smt_store::SMTStateStore;
use crate::snapshot::StoreSnapshot;
use crate::traits::kv_store::KVStoreRead;
use crate::traits::kv_store::KVStoreWrite;
use anyhow::Result;
use gw_common::{error::Error as StateError, smt::SMT, state::State, H256};
use gw_traits::CodeStore;
use gw_types::from_box_should_be_ok;
use gw_types::{
    bytes::Bytes,
    packed::{self, AccountMerkleState},
    prelude::*,
};

use super::mem_store::MemStore;

pub struct MemStateTree {
    tree: SMT<SMTStateStore<MemStore<StoreSnapshot>>>,
    account_count: u32,
}

impl MemStateTree {
    pub fn new(tree: SMT<SMTStateStore<MemStore<StoreSnapshot>>>, account_count: u32) -> Self {
        MemStateTree {
            tree,
            account_count,
        }
    }

    pub fn get_merkle_state(&self) -> AccountMerkleState {
        AccountMerkleState::new_builder()
            .merkle_root(self.tree.root().pack())
            .count(self.account_count.pack())
            .build()
    }

    pub fn smt(&self) -> &SMT<SMTStateStore<MemStore<StoreSnapshot>>> {
        &self.tree
    }

    fn db(&self) -> &SMTStateStore<MemStore<StoreSnapshot>> {
        self.tree.store()
    }
}

impl Clone for MemStateTree {
    fn clone(&self) -> Self {
        Self {
            tree: SMT::new(*self.tree.root(), self.tree.store().clone()),
            account_count: self.account_count,
        }
    }
}

impl State for MemStateTree {
    fn get_raw(&self, key: &H256) -> Result<H256, StateError> {
        let v = self.tree.get(key)?;
        Ok(v)
    }

    fn update_raw(&mut self, key: H256, value: H256) -> Result<(), StateError> {
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

impl CodeStore for MemStateTree {
    fn insert_script(&mut self, script_hash: H256, script: packed::Script) {
        self.db()
            .inner_store()
            .insert_raw(COLUMN_SCRIPT, script_hash.as_slice(), script.as_slice())
            .expect("insert script");
    }

    fn get_script(&self, script_hash: &H256) -> Option<packed::Script> {
        self.db()
            .inner_store()
            .get(COLUMN_SCRIPT, script_hash.as_slice())
            .map(|slice| from_box_should_be_ok!(packed::ScriptReader, slice))
    }

    fn insert_data(&mut self, data_hash: H256, code: Bytes) {
        self.db()
            .inner_store()
            .insert_raw(COLUMN_DATA, data_hash.as_slice(), &code)
            .expect("insert data");
    }

    fn get_data(&self, data_hash: &H256) -> Option<Bytes> {
        self.db()
            .inner_store()
            .get(COLUMN_DATA, data_hash.as_slice())
            .map(|slice| Bytes::from(slice.to_vec()))
    }
}
