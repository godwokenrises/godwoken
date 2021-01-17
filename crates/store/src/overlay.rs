//! Provide overlay store feature
//! Overlay store can be abandoned or commited.

use crate::{CodeStore, Store};
use anyhow::Result;
use gw_common::{
    error::Error,
    smt::SMT,
    sparse_merkle_tree::{
        error::Error as SMTError,
        traits::Store as SMTStore,
        tree::{BranchNode, LeafNode},
        H256,
    },
    state::State,
};
use gw_types::{bytes::Bytes, packed::Script};
use std::collections::{HashMap, HashSet};

pub struct OverlayStore {
    tree: SMT<OverlaySMTStore>,
    store: Store,
    scripts: HashMap<H256, Script>,
    codes: HashMap<H256, Bytes>,
    account_count: u32,
}

impl OverlayStore {
    pub fn new(root: H256, store: Store, account_count: u32) -> Self {
        let smt_store = OverlaySMTStore::new(store.clone());
        let tree = SMT::new(root, smt_store);
        OverlayStore {
            tree,
            store,
            account_count,
            scripts: Default::default(),
            codes: Default::default(),
        }
    }

    pub fn overlay_store(&self) -> &OverlaySMTStore {
        self.tree.store()
    }

    pub fn overlay_store_mut(&mut self) -> &mut OverlaySMTStore {
        self.tree.store_mut()
    }
}

impl State for OverlayStore {
    fn get_raw(&self, key: &H256) -> Result<H256, Error> {
        let v = self.tree.get(&(*key).into())?;
        Ok(v.into())
    }
    fn update_raw(&mut self, key: H256, value: H256) -> Result<(), Error> {
        self.tree.update(key.into(), value.into())?;
        Ok(())
    }
    fn calculate_root(&self) -> Result<H256, Error> {
        let root = (*self.tree.root()).into();
        Ok(root)
    }
    fn get_account_count(&self) -> Result<u32, Error> {
        Ok(self.account_count)
    }
    fn set_account_count(&mut self, count: u32) -> Result<(), Error> {
        self.account_count = count;
        Ok(())
    }
}

impl CodeStore for OverlayStore {
    fn insert_script(&mut self, script_hash: H256, script: Script) {
        self.scripts.insert(script_hash.into(), script);
    }
    fn get_script(&self, script_hash: &H256) -> Option<Script> {
        match self.scripts.get(script_hash) {
            Some(script) => Some(script.clone()),
            None => {
                let db = self.store.begin_transaction();
                let tree = db.account_state_tree().expect("smt store");
                tree.get_script(script_hash)
            }
        }
    }
    fn insert_data(&mut self, script_hash: H256, code: Bytes) {
        self.codes.insert(script_hash, code);
    }
    fn get_data(&self, data_hash: &H256) -> Option<Bytes> {
        match self.codes.get(data_hash) {
            Some(data) => Some(data.clone()),
            None => {
                let db = self.store.begin_transaction();
                let tree = db.account_state_tree().expect("smt store");
                tree.get_data(data_hash)
            }
        }
    }
}

pub struct OverlaySMTStore {
    store: Store,
    branches_map: HashMap<H256, BranchNode>,
    leaves_map: HashMap<H256, LeafNode<H256>>,
    deleted_branches: HashSet<H256>,
    deleted_leaves: HashSet<H256>,
    touched_keys: HashSet<H256>,
}

impl OverlaySMTStore {
    pub fn new(store: Store) -> Self {
        OverlaySMTStore {
            store,
            branches_map: HashMap::default(),
            leaves_map: HashMap::default(),
            deleted_branches: HashSet::default(),
            deleted_leaves: HashSet::default(),
            touched_keys: HashSet::default(),
        }
    }

    pub fn touched_keys(&self) -> &HashSet<H256> {
        &self.touched_keys
    }

    pub fn clear_touched_keys(&mut self) {
        self.touched_keys.clear()
    }
}

impl SMTStore<H256> for OverlaySMTStore {
    fn get_branch(&self, node: &H256) -> Result<Option<BranchNode>, SMTError> {
        if self.deleted_branches.contains(&node) {
            return Ok(None);
        }
        match self.branches_map.get(node) {
            Some(value) => Ok(Some(value.clone())),
            None => {
                let db = self.store.begin_transaction();
                let smt_store = db
                    .account_smt_store()
                    .map_err(|err| SMTError::Store(format!("{}", err)))?;
                smt_store.get_branch(node)
            }
        }
    }
    fn get_leaf(&self, leaf_hash: &H256) -> Result<Option<LeafNode<H256>>, SMTError> {
        if self.deleted_leaves.contains(&leaf_hash) {
            return Ok(None);
        }
        match self.leaves_map.get(leaf_hash) {
            Some(value) => Ok(Some(value.clone())),
            None => {
                let db = self.store.begin_transaction();
                let smt_store = db
                    .account_smt_store()
                    .map_err(|err| SMTError::Store(format!("{}", err)))?;
                smt_store.get_leaf(leaf_hash)
            }
        }
    }
    fn insert_branch(&mut self, node: H256, branch: BranchNode) -> Result<(), SMTError> {
        self.deleted_branches.remove(&node);
        self.branches_map.insert(node, branch);
        Ok(())
    }
    fn insert_leaf(&mut self, leaf_hash: H256, leaf: LeafNode<H256>) -> Result<(), SMTError> {
        self.deleted_leaves.remove(&leaf_hash);
        self.leaves_map.insert(leaf_hash, leaf);
        self.touched_keys.insert(leaf_hash);
        Ok(())
    }
    fn remove_branch(&mut self, node: &H256) -> Result<(), SMTError> {
        self.deleted_branches.insert(*node);
        self.branches_map.remove(node);
        Ok(())
    }
    fn remove_leaf(&mut self, leaf_hash: &H256) -> Result<(), SMTError> {
        self.deleted_leaves.insert(*leaf_hash);
        self.leaves_map.remove(leaf_hash);
        self.touched_keys.insert(*leaf_hash);
        Ok(())
    }
}
