use std::sync::Arc;

use gw_common::{
    smt::{Store, SMT},
    H256,
};
use gw_db::{
    error::Error,
    schema::{
        COLUMN_MEM_POOL_TRANSACTION, COLUMN_MEM_POOL_TRANSACTION_RECEIPT,
        COLUMN_MEM_POOL_WITHDRAWAL,
    },
};
use gw_types::{packed, prelude::*};

use super::StoreTransaction;
use crate::{
    mem_pool_store::{
        MemPoolStore, Value, MEM_POOL_COLUMNS, MEM_POOL_COL_META, META_MEM_POOL_BLOCK_INFO,
    },
    smt::{mem_pool_smt_store::MemPoolSMTStore, mem_smt_store::MemSMTStore},
    state::{
        mem_pool_state_db::MemPoolStateTree,
        mem_state_db::{MemStateContext, MemStateTree},
    },
    traits::KVStore,
};

impl StoreTransaction {
    /// Used for package new mem block
    pub fn in_mem_state_tree<S: Store<H256>>(
        &self,
        smt_store: S,
        context: MemStateContext,
    ) -> Result<MemStateTree<S>, Error> {
        let block = self.get_last_valid_tip_block()?;
        let merkle_root = block.raw().post_account();
        let account_count = merkle_root.count().unpack();
        let block_post_count: u32 = block.raw().post_account().count().unpack();
        log::debug!(
            "Start in mem state account {} block count {} is_same: {}",
            account_count,
            block_post_count,
            account_count == block_post_count
        );
        let mem_smt_store = MemSMTStore::new(smt_store);
        let tree = SMT::new(merkle_root.merkle_root().unpack(), mem_smt_store);
        Ok(MemStateTree::new(self, tree, account_count, context))
    }

    pub fn mem_pool_account_smt(&self) -> Result<MemPoolSMTStore<'_>, Error> {
        Ok(MemPoolSMTStore::new(self, self.mem_pool.load().clone()))
    }

    pub fn mem_pool_state_tree(&self) -> Result<MemPoolStateTree, Error> {
        let (root, count) = match self.get_mem_block_account_smt_root()? {
            Some(root) => {
                let count = self
                    .get_mem_block_account_count()?
                    .expect("get mem pool account count");
                (root, count)
            }
            None => {
                let merkle = self.get_last_valid_tip_block()?.raw().post_account();
                (merkle.merkle_root().unpack(), merkle.count().unpack())
            }
        };
        let smt_store = self.mem_pool_account_smt()?;
        let tree = SMT::new(root, smt_store);
        Ok(MemPoolStateTree::new(tree, count))
    }

    pub fn clear_mem_block_state(&self) -> Result<(), Error> {
        let mem_pool_store = MemPoolStore::new(MEM_POOL_COLUMNS);
        self.mem_pool.store(Arc::new(mem_pool_store));
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
        self.mem_pool.load().insert(
            MEM_POOL_COL_META,
            META_MEM_POOL_BLOCK_INFO.into(),
            Value::Exist(block_info.as_slice().to_vec().into()),
        );
        Ok(())
    }

    pub fn get_mem_pool_block_info(&self) -> Result<Option<packed::BlockInfo>, Error> {
        Ok(self
            .mem_pool
            .load()
            .get(MEM_POOL_COL_META, META_MEM_POOL_BLOCK_INFO)
            .and_then(|v| v.to_opt())
            .map(|slice| {
                packed::BlockInfoReader::from_slice_should_be_ok(slice.as_ref()).to_entity()
            }))
    }
}
