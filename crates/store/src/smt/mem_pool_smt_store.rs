//! Implement SMTStore trait

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

const DELETED_FLAG: u8 = 0;

/// MemPool SMTStore
/// This is a mem-pool layer build upon SMTStore
pub struct MemPoolSMTStore<'a> {
    store: &'a StoreTransaction,
    mem_pool_columns: Columns,
    under_layer_columns: Columns,
}

impl<'a> MemPoolSMTStore<'a> {
    pub fn new(
        mem_pool_columns: Columns,
        under_layer_columns: Columns,
        store: &'a StoreTransaction,
    ) -> Self {
        MemPoolSMTStore {
            mem_pool_columns,
            under_layer_columns,
            store,
        }
    }

    pub fn inner_store(&self) -> &StoreTransaction {
        self.store
    }
}

impl<'a> Store<H256> for MemPoolSMTStore<'a> {
    fn get_branch(&self, branch_key: &BranchKey) -> Result<Option<BranchNode>, SMTError> {
        let branch_key: packed::SMTBranchKey = branch_key.pack();
        let node_slice = match self
            .inner_store()
            .get(self.mem_pool_columns.branch_col, branch_key.as_slice())
        {
            Some(slice) if slice.as_ref() == [DELETED_FLAG] => return Ok(None),
            Some(slice) => slice,
            None => match self
                .inner_store()
                .get(self.under_layer_columns.branch_col, branch_key.as_slice())
            {
                Some(slice) if slice.as_ref() == [DELETED_FLAG] => return Ok(None),
                Some(slice) => slice,
                None => return Ok(None),
            },
        };

        let branch = packed::SMTBranchNodeReader::from_slice_should_be_ok(node_slice.as_ref());
        Ok(Some(branch.to_entity().unpack()))
    }

    fn get_leaf(&self, leaf_key: &H256) -> Result<Option<H256>, SMTError> {
        let leaf_slice = match self
            .inner_store()
            .get(self.mem_pool_columns.leaf_col, leaf_key.as_slice())
        {
            Some(slice) if slice.as_ref() == [DELETED_FLAG] => return Ok(None),
            Some(slice) => slice,
            None => match self
                .inner_store()
                .get(self.under_layer_columns.leaf_col, leaf_key.as_slice())
            {
                Some(slice) if slice.as_ref() == [DELETED_FLAG] => return Ok(None),
                Some(slice) => slice,
                None => return Ok(None),
            },
        };

        if 32 != leaf_slice.len() {
            return Err(SMTError::Store("get crrupted leaf".to_string()));
        }

        let mut leaf = [0u8; 32];
        leaf.copy_from_slice(leaf_slice.as_ref());
        Ok(Some(H256::from(leaf)))
    }

    fn insert_branch(&mut self, branch_key: BranchKey, branch: BranchNode) -> Result<(), SMTError> {
        let branch_key: packed::SMTBranchKey = branch_key.pack();
        let branch: packed::SMTBranchNode = branch.pack();

        self.store
            .insert_raw(
                self.mem_pool_columns.branch_col,
                branch_key.as_slice(),
                branch.as_slice(),
            )
            .map_err(|err| SMTError::Store(format!("insert error {}", err)))?;

        Ok(())
    }

    fn insert_leaf(&mut self, leaf_key: H256, leaf: H256) -> Result<(), SMTError> {
        self.store
            .insert_raw(
                self.mem_pool_columns.leaf_col,
                leaf_key.as_slice(),
                leaf.as_slice(),
            )
            .map_err(|err| SMTError::Store(format!("insert error {}", err)))?;

        Ok(())
    }

    fn remove_branch(&mut self, branch_key: &BranchKey) -> Result<(), SMTError> {
        let branch_key: packed::SMTBranchKey = branch_key.pack();

        self.store
            .insert_raw(
                self.mem_pool_columns.branch_col,
                branch_key.as_slice(),
                &[DELETED_FLAG],
            )
            .map_err(|err| SMTError::Store(format!("delete error {}", err)))?;

        Ok(())
    }

    fn remove_leaf(&mut self, leaf_key: &H256) -> Result<(), SMTError> {
        self.store
            .insert_raw(
                self.mem_pool_columns.leaf_col,
                leaf_key.as_slice(),
                &[DELETED_FLAG],
            )
            .map_err(|err| SMTError::Store(format!("delete error {}", err)))?;

        Ok(())
    }
}
