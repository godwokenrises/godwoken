//! Provide snapshot feature

use crate::transaction::StoreTransaction;
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
use gw_db::error::Error as DBError;
use gw_traits::{ChainStore, CodeStore};
use gw_types::{
    bytes::Bytes,
    packed::{self, Script},
};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

/// SnapshotKey
/// Represent a history point of the db.
/// In the db representation we convert it to prefix key: <raw-key>-<block-number>-<tx-index>
pub enum SnapshotKey {
    // get snapshot of db after processed the block
    AtBlock(H256),
    // get snapshot of db after processed the transaction
    AtTx { block_hash: H256, tx_index: u32 },
}

/// Represent a history point,
/// changes on Snapshot won't affect the main chain db
pub struct Snapshot {
    tree: SMT<OverlaySMTStore>,
    db: Rc<StoreTransaction>,
    scripts: HashMap<H256, Script>,
    codes: HashMap<H256, Bytes>,
    account_count: u32,
}

impl Snapshot {
    /// Get snapshot at a history point
    ///
    /// - db, StoreTransaction that contains all main chain state
    /// - key, Represents a history point
    pub fn storage_at(db: Rc<StoreTransaction>, _key: SnapshotKey) -> Result<Self> {
        let root = db.get_account_smt_root()?;
        let account_count = db.get_account_count()?;
        let smt_store = OverlaySMTStore::new(db.clone());
        let tree = SMT::new(root, smt_store);
        let overlay_store = Snapshot {
            tree,
            db,
            account_count,
            scripts: Default::default(),
            codes: Default::default(),
        };
        Ok(overlay_store)
    }

    pub fn state_tree_touched_keys(&self) -> &HashSet<H256> {
        self.tree.store().touched_keys()
    }
}

impl State for Snapshot {
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

impl CodeStore for Snapshot {
    fn insert_script(&mut self, script_hash: H256, script: Script) {
        self.scripts.insert(script_hash.into(), script);
    }
    fn get_script(&self, script_hash: &H256) -> Option<Script> {
        match self.scripts.get(script_hash) {
            Some(script) => Some(script.clone()),
            None => {
                let tree = self.db.account_state_tree().expect("smt store");
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
                let tree = self.db.account_state_tree().expect("smt store");
                tree.get_data(data_hash)
            }
        }
    }
}

impl ChainStore for Snapshot {
    fn get_tip_block_hash(&self) -> Result<H256, DBError> {
        self.db.get_tip_block_hash()
    }
    fn get_block_hash_by_number(&self, number: u64) -> Result<Option<H256>, DBError> {
        self.db.get_block_hash_by_number(number)
    }
    fn get_block_number(&self, hash: &H256) -> Result<Option<u64>, DBError> {
        self.db.get_block_number(hash)
    }
    fn get_block_by_number(&self, number: u64) -> Result<Option<packed::L2Block>, DBError> {
        self.db.get_block_by_number(number)
    }
    fn get_block(&self, block_hash: &H256) -> Result<Option<packed::L2Block>, DBError> {
        self.db.get_block(block_hash)
    }
    fn get_transaction(&self, tx_hash: &H256) -> Result<Option<packed::L2Transaction>, DBError> {
        self.db.get_transaction(tx_hash)
    }
}

struct OverlaySMTStore {
    db: Rc<StoreTransaction>,
    branches_map: HashMap<H256, BranchNode>,
    leaves_map: HashMap<H256, LeafNode<H256>>,
    deleted_branches: HashSet<H256>,
    deleted_leaves: HashSet<H256>,
    touched_keys: HashSet<H256>,
}

impl OverlaySMTStore {
    fn new(db: Rc<StoreTransaction>) -> Self {
        OverlaySMTStore {
            db,
            branches_map: HashMap::default(),
            leaves_map: HashMap::default(),
            deleted_branches: HashSet::default(),
            deleted_leaves: HashSet::default(),
            touched_keys: HashSet::default(),
        }
    }

    fn touched_keys(&self) -> &HashSet<H256> {
        &self.touched_keys
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
                let smt_store = self
                    .db
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
                let smt_store = self
                    .db
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
