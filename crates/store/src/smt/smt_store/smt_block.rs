//! Implement SMTStore trait

use std::convert::TryInto;

use crate::traits::{chain_store::ChainStore, kv_store::KVStore};
use gw_db::schema::{COLUMN_BLOCK_SMT_BRANCH, COLUMN_BLOCK_SMT_LEAF};
use gw_smt::{
    smt::{SMT, SMTH256},
    smt_h256_ext::SMTH256Ext,
    sparse_merkle_tree::{
        error::Error as SMTError,
        traits::{StoreReadOps, StoreWriteOps},
        BranchKey, BranchNode,
    },
};

use crate::smt::serde::{branch_key_to_vec, branch_node_to_vec, slice_to_branch_node};

pub struct SMTBlockStore<DB: KVStore>(DB);

impl<DB: KVStore + ChainStore> SMTBlockStore<DB> {
    pub fn to_smt(self) -> anyhow::Result<SMT<Self>> {
        let root = self.inner_store().get_block_smt_root()?;
        Ok(SMT::new(SMTH256::from_h256(root), self))
    }
}

impl<DB: KVStore> SMTBlockStore<DB> {
    pub fn new(store: DB) -> Self {
        SMTBlockStore(store)
    }

    pub fn inner_store(&self) -> &DB {
        &self.0
    }
}

impl<DB: KVStore> StoreReadOps<SMTH256> for SMTBlockStore<DB> {
    fn get_branch(&self, branch_key: &BranchKey) -> Result<Option<BranchNode>, SMTError> {
        match self
            .0
            .get(COLUMN_BLOCK_SMT_BRANCH, &branch_key_to_vec(branch_key))
        {
            Some(slice) => Ok(Some(slice_to_branch_node(&slice))),
            None => Ok(None),
        }
    }

    fn get_leaf(&self, leaf_key: &SMTH256) -> Result<Option<SMTH256>, SMTError> {
        match self.0.get(COLUMN_BLOCK_SMT_LEAF, leaf_key.as_slice()) {
            Some(slice) if 32 == slice.len() => {
                let leaf: [u8; 32] = slice.as_ref().try_into().unwrap();
                Ok(Some(SMTH256::from(leaf)))
            }
            Some(_) => Err(SMTError::Store("get corrupted leaf".to_string())),
            None => Ok(None),
        }
    }
}

impl<DB: KVStore> StoreWriteOps<SMTH256> for SMTBlockStore<DB> {
    fn insert_branch(&mut self, branch_key: BranchKey, branch: BranchNode) -> Result<(), SMTError> {
        self.0
            .insert_raw(
                COLUMN_BLOCK_SMT_BRANCH,
                &branch_key_to_vec(&branch_key),
                &branch_node_to_vec(&branch),
            )
            .map_err(|err| SMTError::Store(format!("insert error {}", err)))?;

        Ok(())
    }

    fn insert_leaf(&mut self, leaf_key: SMTH256, leaf: SMTH256) -> Result<(), SMTError> {
        self.0
            .insert_raw(COLUMN_BLOCK_SMT_LEAF, leaf_key.as_slice(), leaf.as_slice())
            .map_err(|err| SMTError::Store(format!("insert error {}", err)))?;

        Ok(())
    }

    fn remove_branch(&mut self, branch_key: &BranchKey) -> Result<(), SMTError> {
        self.0
            .delete(COLUMN_BLOCK_SMT_BRANCH, &branch_key_to_vec(branch_key))
            .map_err(|err| SMTError::Store(format!("delete error {}", err)))?;

        Ok(())
    }

    fn remove_leaf(&mut self, leaf_key: &SMTH256) -> Result<(), SMTError> {
        self.0
            .delete(COLUMN_BLOCK_SMT_LEAF, leaf_key.as_slice())
            .map_err(|err| SMTError::Store(format!("delete error {}", err)))?;

        Ok(())
    }
}
