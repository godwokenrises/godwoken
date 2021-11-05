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
use gw_db::schema::Col;
use gw_types::{packed, prelude::*};

const DELETED_FLAG: u8 = 0;

pub struct Columns {
    pub leaf_col: Col,
    pub branch_col: Col,
}

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
        &self.store
    }
}

impl<'a> Store<H256> for MemPoolSMTStore<'a> {
    fn get_branch(&self, branch_key: &BranchKey) -> Result<Option<BranchNode>, SMTError> {
        let branch_key: packed::SMTBranchKey = branch_key.pack();
        let opt = self
            .inner_store()
            .get(self.mem_pool_columns.branch_col, branch_key.as_slice())
            .filter(|slice| slice.as_ref() != &[DELETED_FLAG])
            .or_else(|| {
                self.inner_store()
                    .get(self.under_layer_columns.branch_col, branch_key.as_slice())
            })
            .map(|slice| {
                let branch = packed::SMTBranchNodeReader::from_slice_should_be_ok(slice.as_ref());
                branch.to_entity().unpack()
            });
        Ok(opt)
    }

    fn get_leaf(&self, leaf_key: &H256) -> Result<Option<H256>, SMTError> {
        match self
            .store
            .get(self.mem_pool_columns.leaf_col, leaf_key.as_slice())
            .filter(|slice| slice.as_ref() != &[DELETED_FLAG])
            .or_else(|| {
                self.store
                    .get(self.under_layer_columns.leaf_col, leaf_key.as_slice())
            }) {
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
