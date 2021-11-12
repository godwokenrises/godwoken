//! Implement SMTStore trait

use std::collections::HashMap;

use gw_common::{
    sparse_merkle_tree::{
        error::Error as SMTError,
        traits::Store,
        tree::{BranchKey, BranchNode},
    },
    H256,
};
#[derive(Debug, PartialEq, Eq)]
enum Value<V> {
    Deleted,
    Exist(V),
}

#[derive(Debug, Default)]
struct MemStore {
    branches: HashMap<BranchKey, Value<BranchNode>>,
    leaves: HashMap<H256, Value<H256>>,
}

pub struct MemSMTStore<S> {
    store: S,
    mem_store: MemStore,
}

impl<S: Store<H256>> MemSMTStore<S> {
    pub fn new(store: S) -> Self {
        MemSMTStore {
            store,
            mem_store: Default::default(),
        }
    }
}

impl<S: Store<H256>> Store<H256> for MemSMTStore<S> {
    fn get_branch(&self, branch_key: &BranchKey) -> Result<Option<BranchNode>, SMTError> {
        match self.mem_store.branches.get(branch_key) {
            Some(Value::Deleted) => Ok(None),
            Some(Value::Exist(v)) => Ok(Some(v.to_owned())),
            None => self.store.get_branch(branch_key),
        }
    }

    fn get_leaf(&self, leaf_key: &H256) -> Result<Option<H256>, SMTError> {
        match self.mem_store.leaves.get(leaf_key) {
            Some(Value::Deleted) => Ok(None),
            Some(Value::Exist(v)) => Ok(Some(v.to_owned())),
            None => self.store.get_leaf(leaf_key),
        }
    }

    fn insert_branch(&mut self, branch_key: BranchKey, branch: BranchNode) -> Result<(), SMTError> {
        self.mem_store
            .branches
            .insert(branch_key, Value::Exist(branch));
        Ok(())
    }

    fn insert_leaf(&mut self, leaf_key: H256, leaf: H256) -> Result<(), SMTError> {
        self.mem_store.leaves.insert(leaf_key, Value::Exist(leaf));
        Ok(())
    }

    fn remove_branch(&mut self, branch_key: &BranchKey) -> Result<(), SMTError> {
        self.mem_store
            .branches
            .insert(branch_key.to_owned(), Value::Deleted);

        Ok(())
    }

    fn remove_leaf(&mut self, leaf_key: &H256) -> Result<(), SMTError> {
        self.mem_store.leaves.insert(*leaf_key, Value::Deleted);
        Ok(())
    }
}
