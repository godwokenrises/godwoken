//! Implement SMTStore trait

use crate::traits::KVStore;
use gw_common::{
    sparse_merkle_tree::{
        error::Error as SMTError,
        traits::Store,
        tree::{BranchNode, LeafNode},
    },
    H256,
};
use gw_db::schema::Col;
use gw_types::{packed, prelude::*};

pub struct SMTStore<'a, DB: KVStore> {
    leaf_col: Col<'a>,
    branch_col: Col<'a>,
    store: &'a DB,
}

impl<'a, DB: KVStore> SMTStore<'a, DB> {
    pub fn new(leaf_col: Col<'a>, branch_col: Col<'a>, store: &'a DB) -> Self {
        SMTStore {
            leaf_col,
            branch_col,
            store,
        }
    }
}

impl<'a, DB: KVStore> Store<H256> for SMTStore<'a, DB> {
    fn get_branch(&self, node: &H256) -> Result<Option<BranchNode>, SMTError> {
        match self.store.get(self.branch_col, node.as_slice()) {
            Some(slice) => {
                let branch = packed::SMTBranchNodeReader::from_slice_should_be_ok(&slice.as_ref())
                    .to_entity();
                Ok(Some(branch.unpack()))
            }
            None => Ok(None),
        }
    }
    fn get_leaf(&self, leaf_hash: &H256) -> Result<Option<LeafNode<H256>>, SMTError> {
        match self.store.get(self.leaf_col, leaf_hash.as_slice()) {
            Some(slice) => {
                let leaf =
                    packed::SMTLeafNodeReader::from_slice_should_be_ok(&slice.as_ref()).to_entity();
                Ok(Some(leaf.unpack()))
            }
            None => Ok(None),
        }
    }
    fn insert_branch(&mut self, node: H256, branch: BranchNode) -> Result<(), SMTError> {
        let branch: packed::SMTBranchNode = branch.pack();
        self.store
            .insert_raw(self.branch_col, node.as_slice(), branch.as_slice())
            .map_err(|err| SMTError::Store(format!("Insert error {}", err)))?;
        Ok(())
    }
    fn insert_leaf(&mut self, leaf_hash: H256, leaf: LeafNode<H256>) -> Result<(), SMTError> {
        let leaf: packed::SMTLeafNode = leaf.pack();
        self.store
            .insert_raw(self.leaf_col, leaf_hash.as_slice(), leaf.as_slice())
            .map_err(|err| SMTError::Store(format!("Insert error {}", err)))?;
        Ok(())
    }
    fn remove_branch(&mut self, node: &H256) -> Result<(), SMTError> {
        self.store
            .delete(self.branch_col, node.as_slice())
            .map_err(|err| SMTError::Store(format!("Delete error {}", err)))?;
        Ok(())
    }
    fn remove_leaf(&mut self, leaf_hash: &H256) -> Result<(), SMTError> {
        self.store
            .delete(self.leaf_col, leaf_hash.as_slice())
            .map_err(|err| SMTError::Store(format!("Delete error {}", err)))?;
        Ok(())
    }
}
