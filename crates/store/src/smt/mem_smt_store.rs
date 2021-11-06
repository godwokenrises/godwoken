//! Implement SMTStore trait

use std::collections::HashMap;

use crate::{traits::KVStore, transaction::StoreTransaction};
use gw_common::{
    sparse_merkle_tree::{
        error::Error as SMTError,
        traits::Store,
        tree::{BranchKey, BranchNode},
    },
    H256,
};
use gw_types::{packed, prelude::*};

use super::Columns;

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

pub struct MemSMTStore<'a> {
    under_layer_columns: Columns,
    store: &'a StoreTransaction,
    mem_store: MemStore,
}

impl<'a> MemSMTStore<'a> {
    pub fn new(under_layer_columns: Columns, store: &'a StoreTransaction) -> Self {
        MemSMTStore {
            under_layer_columns,
            store,
            mem_store: Default::default(),
        }
    }

    pub fn inner_store(&self) -> &StoreTransaction {
        &self.store
    }
}

impl<'a> Store<H256> for MemSMTStore<'a> {
    fn get_branch(&self, branch_key: &BranchKey) -> Result<Option<BranchNode>, SMTError> {
        match self.mem_store.branches.get(&branch_key) {
            Some(Value::Deleted) => Ok(None),
            Some(Value::Exist(v)) => Ok(Some(v.to_owned())),
            None => {
                let branch_key: packed::SMTBranchKey = branch_key.pack();
                let opt = self
                    .store
                    .get(self.under_layer_columns.branch_col, branch_key.as_slice())
                    .map(|slice| {
                        let branch =
                            packed::SMTBranchNodeReader::from_slice_should_be_ok(slice.as_ref());
                        branch.to_entity().unpack()
                    });
                Ok(opt)
            }
        }
    }

    fn get_leaf(&self, leaf_key: &H256) -> Result<Option<H256>, SMTError> {
        match self.mem_store.leaves.get(&leaf_key) {
            Some(Value::Deleted) => Ok(None),
            Some(Value::Exist(v)) => Ok(Some(v.to_owned())),
            None => {
                let opt = self
                    .store
                    .get(self.under_layer_columns.leaf_col, leaf_key.as_slice())
                    .map(|slice| {
                        assert_eq!(slice.len(), 32, "corrupted smt leaf");
                        let mut leaf = [0u8; 32];
                        leaf.copy_from_slice(slice.as_ref());
                        H256::from(leaf)
                    });
                Ok(opt)
            }
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
