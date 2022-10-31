//! Implement SMTStore trait

use std::convert::TryInto;

use crate::traits::{chain_store::ChainStore, kv_store::KVStore};
use gw_common::{
    smt::SMT,
    sparse_merkle_tree::{
        error::Error as SMTError,
        traits::Store,
        tree::{BranchKey, BranchNode},
    },
    H256,
};
use gw_db::schema::{COLUMN_ACCOUNT_SMT_BRANCH, COLUMN_ACCOUNT_SMT_LEAF};
use gw_types::prelude::Unpack;

use crate::smt::serde::{branch_key_to_vec, branch_node_to_vec, slice_to_branch_node};

pub struct SMTStateStore<DB>(DB);

impl<DB: Clone> Clone for SMTStateStore<DB> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<DB: KVStore + ChainStore> SMTStateStore<DB> {
    pub fn to_smt(self) -> anyhow::Result<SMT<Self>> {
        let root = self
            .inner_store()
            .get_last_valid_tip_block()?
            .raw()
            .post_account()
            .merkle_root()
            .unpack();
        Ok(SMT::new(root, self))
    }
}

impl<DB: KVStore> SMTStateStore<DB> {
    pub fn new(store: DB) -> Self {
        Self(store)
    }

    pub fn inner_store(&self) -> &DB {
        &self.0
    }

    pub fn inner_store_mut(&mut self) -> &mut DB {
        &mut self.0
    }
}

impl<DB: KVStore> Store<H256> for SMTStateStore<DB> {
    fn get_branch(&self, branch_key: &BranchKey) -> Result<Option<BranchNode>, SMTError> {
        match self
            .0
            .get(COLUMN_ACCOUNT_SMT_BRANCH, &branch_key_to_vec(branch_key))
        {
            Some(slice) => Ok(Some(slice_to_branch_node(&slice))),
            None => Ok(None),
        }
    }

    fn get_leaf(&self, leaf_key: &H256) -> Result<Option<H256>, SMTError> {
        match self.0.get(COLUMN_ACCOUNT_SMT_LEAF, leaf_key.as_slice()) {
            Some(slice) if 32 == slice.len() => {
                let leaf: [u8; 32] = slice.as_ref().try_into().unwrap();
                Ok(Some(H256::from(leaf)))
            }
            Some(_) => Err(SMTError::Store("get corrupted leaf".to_string())),
            None => Ok(None),
        }
    }

    fn insert_branch(&mut self, branch_key: BranchKey, branch: BranchNode) -> Result<(), SMTError> {
        self.0
            .insert_raw(
                COLUMN_ACCOUNT_SMT_BRANCH,
                &branch_key_to_vec(&branch_key),
                &branch_node_to_vec(&branch),
            )
            .map_err(|err| SMTError::Store(format!("insert error {}", err)))?;

        Ok(())
    }

    fn insert_leaf(&mut self, leaf_key: H256, leaf: H256) -> Result<(), SMTError> {
        self.0
            .insert_raw(
                COLUMN_ACCOUNT_SMT_LEAF,
                leaf_key.as_slice(),
                leaf.as_slice(),
            )
            .map_err(|err| SMTError::Store(format!("insert error {}", err)))?;

        Ok(())
    }

    fn remove_branch(&mut self, branch_key: &BranchKey) -> Result<(), SMTError> {
        self.0
            .delete(COLUMN_ACCOUNT_SMT_BRANCH, &branch_key_to_vec(branch_key))
            .map_err(|err| SMTError::Store(format!("delete error {}", err)))?;

        Ok(())
    }

    fn remove_leaf(&mut self, leaf_key: &H256) -> Result<(), SMTError> {
        self.0
            .delete(COLUMN_ACCOUNT_SMT_LEAF, leaf_key.as_slice())
            .map_err(|err| SMTError::Store(format!("delete error {}", err)))?;

        Ok(())
    }
}
