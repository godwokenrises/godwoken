//! Provide overlay store feature
//! Overlay store can be abandoned or commited.

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
use gw_generator::traits::CodeStore;
use gw_types::{bytes::Bytes, packed::Script};
use std::collections::{HashMap, HashSet};

pub struct OverlayStore<S> {
    tree: SMT<OverlaySMTStore<S>>,
    scripts: HashMap<H256, Script>,
    codes: HashMap<H256, Bytes>,
    account_count: u32,
}

impl<S: SMTStore<H256>> OverlayStore<S> {
    pub fn new(
        root: H256,
        store: OverlaySMTStore<S>,
        account_count: u32,
        scripts: HashMap<H256, Script>,
        codes: HashMap<H256, Bytes>,
    ) -> Self {
        let tree = SMT::new(root, store);
        OverlayStore {
            tree,
            account_count,
            scripts,
            codes,
        }
    }

    pub fn overlay_store(&self) -> &OverlaySMTStore<S> {
        self.tree.store()
    }

    pub fn overlay_store_mut(&mut self) -> &mut OverlaySMTStore<S> {
        self.tree.store_mut()
    }
}

impl<S: SMTStore<H256>> State for OverlayStore<S> {
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

impl<S: SMTStore<H256>> CodeStore for OverlayStore<S> {
    fn insert_script(&mut self, script_hash: H256, script: Script) {
        self.scripts.insert(script_hash.into(), script);
    }
    fn insert_code(&mut self, code_hash: H256, code: Bytes) {
        self.codes.insert(code_hash.into(), code);
    }
    fn get_script(&self, script_hash: &H256) -> Option<Script> {
        self.scripts.get(&script_hash).cloned()
    }
    fn get_code(&self, code_hash: &H256) -> Option<Bytes> {
        self.codes.get(&code_hash).cloned()
    }
}

pub struct OverlaySMTStore<S> {
    store: S,
    branches_map: HashMap<H256, BranchNode>,
    leaves_map: HashMap<H256, LeafNode<H256>>,
    deleted_branches: HashSet<H256>,
    deleted_leaves: HashSet<H256>,
    touched_keys: HashSet<H256>,
}

impl<S: SMTStore<H256>> OverlaySMTStore<S> {
    pub fn new(store: S) -> Self {
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

impl<S: SMTStore<H256>> SMTStore<H256> for OverlaySMTStore<S> {
    fn get_branch(&self, node: &H256) -> Result<Option<BranchNode>, SMTError> {
        if self.deleted_branches.contains(&node) {
            return Ok(None);
        }
        match self.branches_map.get(node) {
            Some(value) => Ok(Some(value.clone())),
            None => self.store.get_branch(node),
        }
    }
    fn get_leaf(&self, leaf_hash: &H256) -> Result<Option<LeafNode<H256>>, SMTError> {
        if self.deleted_leaves.contains(&leaf_hash) {
            return Ok(None);
        }
        match self.leaves_map.get(leaf_hash) {
            Some(value) => Ok(Some(value.clone())),
            None => self.store.get_leaf(leaf_hash),
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
