use gw_common::{smt::SMT, H256};
use gw_db::{
    error::Error,
    schema::{
        COLUMN_ACCOUNT_SMT_BRANCH, COLUMN_ACCOUNT_SMT_LEAF, COLUMN_MEM_POOL_ACCOUNT_SMT_BRANCH,
        COLUMN_MEM_POOL_ACCOUNT_SMT_LEAF, COLUMN_MEM_POOL_DATA, COLUMN_MEM_POOL_SCRIPT,
        COLUMN_MEM_POOL_SCRIPT_PREFIX, COLUMN_MEM_POOL_TRANSACTION,
        COLUMN_MEM_POOL_TRANSACTION_RECEIPT, COLUMN_MEM_POOL_WITHDRAWAL, COLUMN_META,
        META_MEM_POOL_BLOCK_INFO,
    },
    IteratorMode,
};
use gw_types::{packed, prelude::*};

use super::StoreTransaction;
use crate::{
    smt::{mem_pool_smt_store::MemPoolSMTStore, mem_smt_store::MemSMTStore, Columns},
    state::{mem_pool_state_db::MemPoolStateTree, mem_state_db::MemStateTree},
    traits::KVStore,
};

impl StoreTransaction {
    /// Used for package new mem block
    pub fn in_mem_state_tree(&self) -> Result<MemStateTree, Error> {
        let under_layer_columns = Columns {
            leaf_col: COLUMN_ACCOUNT_SMT_LEAF,
            branch_col: COLUMN_ACCOUNT_SMT_BRANCH,
        };
        let block = self.get_tip_block()?;
        let smt_store = MemSMTStore::new(under_layer_columns, self);
        let merkle_root = block.raw().post_account();
        let account_count = self.get_mem_block_account_count()?;
        let tree = SMT::new(merkle_root.merkle_root().unpack(), smt_store);
        Ok(MemStateTree::new(tree, account_count))
    }

    pub fn mem_pool_state_tree(&self) -> Result<MemPoolStateTree, Error> {
        let mem_pool_columns = Columns {
            leaf_col: COLUMN_MEM_POOL_ACCOUNT_SMT_LEAF,
            branch_col: COLUMN_MEM_POOL_ACCOUNT_SMT_BRANCH,
        };
        let under_layer_columns = Columns {
            leaf_col: COLUMN_ACCOUNT_SMT_LEAF,
            branch_col: COLUMN_ACCOUNT_SMT_BRANCH,
        };
        let smt_store = MemPoolSMTStore::new(mem_pool_columns, under_layer_columns, self);
        let merkle_root = self.get_mem_block_account_smt_root()?;
        let account_count = self.get_mem_block_account_count()?;
        let tree = SMT::new(merkle_root, smt_store);
        Ok(MemPoolStateTree::new(tree, account_count))
    }

    pub fn clear_mem_block_state(&self) -> Result<(), Error> {
        for col in [
            COLUMN_MEM_POOL_SCRIPT,
            COLUMN_MEM_POOL_DATA,
            COLUMN_MEM_POOL_SCRIPT_PREFIX,
            COLUMN_MEM_POOL_ACCOUNT_SMT_LEAF,
            COLUMN_MEM_POOL_ACCOUNT_SMT_BRANCH,
        ] {
            for (k, _v) in self.get_iter(col, IteratorMode::Start) {
                self.delete(col, &k)?;
            }
        }
        Ok(())
    }

    pub fn insert_mem_pool_transaction(
        &self,
        tx_hash: &H256,
        tx: packed::L2Transaction,
    ) -> Result<(), Error> {
        self.insert_raw(
            COLUMN_MEM_POOL_TRANSACTION,
            tx_hash.as_slice(),
            tx.as_slice(),
        )
    }

    pub fn get_mem_pool_transaction(
        &self,
        tx_hash: &H256,
    ) -> Result<Option<packed::L2Transaction>, Error> {
        Ok(self
            .get(COLUMN_MEM_POOL_TRANSACTION, tx_hash.as_slice())
            .map(|slice| {
                packed::L2TransactionReader::from_slice_should_be_ok(slice.as_ref()).to_entity()
            }))
    }

    pub fn remove_mem_pool_transaction(&self, tx_hash: &H256) -> Result<(), Error> {
        self.delete(COLUMN_MEM_POOL_TRANSACTION, tx_hash.as_slice())?;
        self.delete(COLUMN_MEM_POOL_TRANSACTION_RECEIPT, tx_hash.as_slice())?;
        Ok(())
    }

    pub fn insert_mem_pool_transaction_receipt(
        &self,
        tx_hash: &H256,
        tx_receipt: packed::TxReceipt,
    ) -> Result<(), Error> {
        self.insert_raw(
            COLUMN_MEM_POOL_TRANSACTION_RECEIPT,
            tx_hash.as_slice(),
            tx_receipt.as_slice(),
        )
    }

    pub fn get_mem_pool_transaction_receipt(
        &self,
        tx_hash: &H256,
    ) -> Result<Option<packed::TxReceipt>, Error> {
        Ok(self
            .get(COLUMN_MEM_POOL_TRANSACTION_RECEIPT, tx_hash.as_slice())
            .map(|slice| {
                packed::TxReceiptReader::from_slice_should_be_ok(slice.as_ref()).to_entity()
            }))
    }

    pub fn insert_mem_pool_withdrawal(
        &self,
        withdrawal_hash: &H256,
        withdrawal: packed::WithdrawalRequest,
    ) -> Result<(), Error> {
        self.insert_raw(
            COLUMN_MEM_POOL_WITHDRAWAL,
            withdrawal_hash.as_slice(),
            withdrawal.as_slice(),
        )
    }

    pub fn get_mem_pool_withdrawal(
        &self,
        withdrawal_hash: &H256,
    ) -> Result<Option<packed::WithdrawalRequest>, Error> {
        Ok(self
            .get(COLUMN_MEM_POOL_WITHDRAWAL, withdrawal_hash.as_slice())
            .map(|slice| {
                packed::WithdrawalRequestReader::from_slice_should_be_ok(slice.as_ref()).to_entity()
            }))
    }

    pub fn remove_mem_pool_withdrawal(&self, withdrawal_hash: &H256) -> Result<(), Error> {
        self.delete(COLUMN_MEM_POOL_WITHDRAWAL, withdrawal_hash.as_slice())?;
        Ok(())
    }

    pub fn update_mem_pool_block_info(&self, block_info: &packed::BlockInfo) -> Result<(), Error> {
        self.insert_raw(COLUMN_META, META_MEM_POOL_BLOCK_INFO, block_info.as_slice())
    }

    pub fn get_mem_pool_block_info(&self) -> Result<Option<packed::BlockInfo>, Error> {
        Ok(self
            .get(COLUMN_META, META_MEM_POOL_BLOCK_INFO)
            .map(|slice| {
                packed::BlockInfoReader::from_slice_should_be_ok(slice.as_ref()).to_entity()
            }))
    }
}
