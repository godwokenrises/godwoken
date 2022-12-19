//! Implement SMTStore trait

use std::convert::TryInto;

use gw_smt::{
    smt::SMT,
    smt_h256_ext::SMTH256,
    sparse_merkle_tree::{
        error::Error as SMTError,
        traits::{StoreReadOps, StoreWriteOps},
        BranchKey, BranchNode,
    },
};

use crate::{
    schema::{COLUMN_REVERTED_BLOCK_SMT_BRANCH, COLUMN_REVERTED_BLOCK_SMT_LEAF},
    smt::serde::{branch_key_to_vec, branch_node_to_vec, slice_to_branch_node},
    traits::{chain_store::ChainStore, kv_store::KVStore},
};

pub struct SMTRevertedBlockStore<DB>(DB);

impl<DB: KVStore + ChainStore> SMTRevertedBlockStore<DB> {
    pub fn to_smt(self) -> anyhow::Result<SMT<Self>> {
        let root = self.inner_store().get_reverted_block_smt_root()?;
        Ok(SMT::new(root.into(), self))
    }
}

impl<DB: KVStore> SMTRevertedBlockStore<DB> {
    pub fn new(store: DB) -> Self {
        SMTRevertedBlockStore(store)
    }

    pub fn inner_store(&self) -> &DB {
        &self.0
    }

    pub fn inner_store_mut(&mut self) -> &mut DB {
        &mut self.0
    }
}

impl<DB: KVStore> StoreReadOps<SMTH256> for SMTRevertedBlockStore<DB> {
    fn get_branch(&self, branch_key: &BranchKey) -> Result<Option<BranchNode>, SMTError> {
        match self.0.get(
            COLUMN_REVERTED_BLOCK_SMT_BRANCH,
            &branch_key_to_vec(branch_key),
        ) {
            Some(slice) => Ok(Some(slice_to_branch_node(&slice))),
            None => Ok(None),
        }
    }

    fn get_leaf(&self, leaf_key: &SMTH256) -> Result<Option<SMTH256>, SMTError> {
        match self
            .0
            .get(COLUMN_REVERTED_BLOCK_SMT_LEAF, leaf_key.as_slice())
        {
            Some(slice) if 32 == slice.len() => {
                let leaf: [u8; 32] = slice.as_ref().try_into().unwrap();
                Ok(Some(leaf.into()))
            }
            Some(_) => Err(SMTError::Store("get corrupted leaf".to_string())),
            None => Ok(None),
        }
    }
}

impl<DB: KVStore> StoreWriteOps<SMTH256> for SMTRevertedBlockStore<DB> {
    fn insert_branch(&mut self, branch_key: BranchKey, branch: BranchNode) -> Result<(), SMTError> {
        self.0
            .insert_raw(
                COLUMN_REVERTED_BLOCK_SMT_BRANCH,
                &branch_key_to_vec(&branch_key),
                &branch_node_to_vec(&branch),
            )
            .map_err(|err| SMTError::Store(format!("insert error {}", err)))?;

        Ok(())
    }

    fn insert_leaf(&mut self, leaf_key: SMTH256, leaf: SMTH256) -> Result<(), SMTError> {
        self.0
            .insert_raw(
                COLUMN_REVERTED_BLOCK_SMT_LEAF,
                leaf_key.as_slice(),
                leaf.as_slice(),
            )
            .map_err(|err| SMTError::Store(format!("insert error {}", err)))?;

        Ok(())
    }

    fn remove_branch(&mut self, branch_key: &BranchKey) -> Result<(), SMTError> {
        self.0
            .delete(
                COLUMN_REVERTED_BLOCK_SMT_BRANCH,
                &branch_key_to_vec(branch_key),
            )
            .map_err(|err| SMTError::Store(format!("delete error {}", err)))?;

        Ok(())
    }

    fn remove_leaf(&mut self, leaf_key: &SMTH256) -> Result<(), SMTError> {
        self.0
            .delete(COLUMN_REVERTED_BLOCK_SMT_LEAF, leaf_key.as_slice())
            .map_err(|err| SMTError::Store(format!("delete error {}", err)))?;

        Ok(())
    }
}
