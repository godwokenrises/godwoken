//! Implement SMTStore trait

use crate::traits::KVStore;
use gw_common::{
    sparse_merkle_tree::{
        error::Error as SMTError,
        traits::Store,
        tree::{BranchKey, BranchNode},
    },
    H256,
};
use gw_db::schema::Col;
use gw_types::{packed, prelude::*};

pub struct SMTStore<'a, DB: KVStore> {
    leaf_col: Col,
    branch_col: Col,
    store: &'a DB,
}

impl<'a, DB: KVStore> SMTStore<'a, DB> {
    pub fn new(leaf_col: Col, branch_col: Col, store: &'a DB) -> Self {
        SMTStore {
            leaf_col,
            branch_col,
            store,
        }
    }

    pub fn inner_store(&self) -> &DB {
        &self.store
    }
}

impl<'a, DB: KVStore> Store<H256> for SMTStore<'a, DB> {
    fn get_branch(&self, branch_key: &BranchKey) -> Result<Option<BranchNode>, SMTError> {
        let branch_key: packed::SMTBranchKey = branch_key.pack();
        match self.store.get(self.branch_col, branch_key.as_slice()) {
            Some(slice) => {
                let branch = packed::SMTBranchNodeReader::from_slice_should_be_ok(slice.as_ref());
                Ok(Some(branch.to_entity().unpack()))
            }
            None => Ok(None),
        }
    }

    fn get_leaf(&self, leaf_key: &H256) -> Result<Option<H256>, SMTError> {
        match self.store.get(self.leaf_col, leaf_key.as_slice()) {
            Some(slice) if 32 == slice.len() => {
                let mut leaf = [0u8; 32];
                leaf.copy_from_slice(slice.as_ref());
                Ok(Some(H256::from(leaf)))
            }
            Some(_) => Err(SMTError::Store("get corrupted leaf".to_string())),
            None => Ok(None),
        }
    }

    fn insert_branch(&mut self, branch_key: BranchKey, branch: BranchNode) -> Result<(), SMTError> {
        let branch_key: packed::SMTBranchKey = branch_key.pack();
        let branch: packed::SMTBranchNode = branch.pack();

        self.store
            .insert_raw(self.branch_col, branch_key.as_slice(), branch.as_slice())
            .map_err(|err| SMTError::Store(format!("insert error {}", err)))?;

        Ok(())
    }

    fn insert_leaf(&mut self, leaf_key: H256, leaf: H256) -> Result<(), SMTError> {
        self.store
            .insert_raw(self.leaf_col, leaf_key.as_slice(), leaf.as_slice())
            .map_err(|err| SMTError::Store(format!("insert error {}", err)))?;

        Ok(())
    }

    fn remove_branch(&mut self, branch_key: &BranchKey) -> Result<(), SMTError> {
        let branch_key: packed::SMTBranchKey = branch_key.pack();

        self.store
            .delete(self.branch_col, branch_key.as_slice())
            .map_err(|err| SMTError::Store(format!("delete error {}", err)))?;

        Ok(())
    }

    fn remove_leaf(&mut self, leaf_key: &H256) -> Result<(), SMTError> {
        self.store
            .delete(self.leaf_col, leaf_key.as_slice())
            .map_err(|err| SMTError::Store(format!("delete error {}", err)))?;

        Ok(())
    }
}
