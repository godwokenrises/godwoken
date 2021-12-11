//! Implement SMTStore trait

use std::sync::Arc;

use crate::{
    mem_pool_store::{MemPoolStore, Value, MEM_POOL_COL_SMT_BRANCH, MEM_POOL_COL_SMT_LEAF},
    traits::KVStore,
    transaction::StoreTransaction,
};
use gw_common::{
    sparse_merkle_tree::{
        error::Error as SMTError,
        traits::Store,
        tree::{BranchKey, BranchNode},
    },
    H256,
};
use gw_db::schema::{COLUMN_ACCOUNT_SMT_BRANCH, COLUMN_ACCOUNT_SMT_LEAF};

use super::serde::{branch_key_to_vec, branch_node_to_vec, slice_to_branch_node};

/// MemPool SMTStore
/// This is a mem-pool layer build upon SMTStore
pub struct MemPoolSMTStore<'a> {
    store: &'a StoreTransaction,
    mem_pool: Arc<MemPoolStore>,
}

impl<'a> MemPoolSMTStore<'a> {
    pub fn new(store: &'a StoreTransaction, mem_pool: Arc<MemPoolStore>) -> Self {
        MemPoolSMTStore { store, mem_pool }
    }

    pub fn inner_store(&self) -> &StoreTransaction {
        self.store
    }
}

impl<'a> Store<H256> for MemPoolSMTStore<'a> {
    fn get_branch(&self, branch_key: &BranchKey) -> Result<Option<BranchNode>, SMTError> {
        let branch_key = branch_key_to_vec(branch_key);
        let opt = match self
            .mem_pool
            .get(MEM_POOL_COL_SMT_BRANCH, branch_key.as_slice())
        {
            Some(Value::Deleted) => None,
            Some(Value::Exist(v)) => Some(v),
            None => self
                .inner_store()
                .get(COLUMN_ACCOUNT_SMT_BRANCH, branch_key.as_slice())
                .map(Into::into),
        };
        Ok(opt.map(|slice| slice_to_branch_node(slice.as_ref())))
    }

    fn get_leaf(&self, leaf_key: &H256) -> Result<Option<H256>, SMTError> {
        let opt = match self
            .mem_pool
            .get(MEM_POOL_COL_SMT_LEAF, leaf_key.as_slice())
        {
            Some(Value::Deleted) => None,
            Some(Value::Exist(v)) => Some(v),
            None => self
                .inner_store()
                .get(COLUMN_ACCOUNT_SMT_LEAF, leaf_key.as_slice())
                .map(Into::into),
        };
        Ok(opt.map(|slice| {
            let mut leaf = [0u8; 32];
            leaf.copy_from_slice(slice.as_ref());
            H256::from(leaf)
        }))
    }

    fn insert_branch(&mut self, branch_key: BranchKey, branch: BranchNode) -> Result<(), SMTError> {
        let branch_key = branch_key_to_vec(&branch_key);
        let branch = branch_node_to_vec(&branch);

        self.mem_pool.insert(
            MEM_POOL_COL_SMT_BRANCH,
            branch_key.into(),
            Value::Exist(branch.into()),
        );

        Ok(())
    }

    fn insert_leaf(&mut self, leaf_key: H256, leaf: H256) -> Result<(), SMTError> {
        self.mem_pool.insert(
            MEM_POOL_COL_SMT_LEAF,
            leaf_key.as_slice().to_vec().into(),
            Value::Exist(leaf.as_slice().to_vec().into()),
        );
        Ok(())
    }

    fn remove_branch(&mut self, branch_key: &BranchKey) -> Result<(), SMTError> {
        let branch_key = branch_key_to_vec(branch_key);

        self.mem_pool
            .insert(MEM_POOL_COL_SMT_BRANCH, branch_key.into(), Value::Deleted);
        Ok(())
    }

    fn remove_leaf(&mut self, leaf_key: &H256) -> Result<(), SMTError> {
        self.mem_pool.insert(
            MEM_POOL_COL_SMT_LEAF,
            leaf_key.as_slice().to_vec().into(),
            Value::Deleted,
        );
        Ok(())
    }
}
