use gw_common::H256;
use gw_db::{
    error::Error,
    schema::{
        COLUMN_MEM_POOL_TRANSACTION, COLUMN_MEM_POOL_TRANSACTION_RECEIPT,
        COLUMN_MEM_POOL_WITHDRAWAL, COLUMN_META, META_MEM_POOL_BLOCK_INFO,
    },
};
use gw_types::{packed, prelude::*};

use super::StoreTransaction;
use crate::{constant::MEMORY_BLOCK_NUMBER, traits::KVStore};

pub trait MemPoolStore {
    fn insert_mem_pool_transaction(
        &self,
        tx_hash: &H256,
        tx: packed::L2Transaction,
    ) -> Result<(), Error>;

    fn get_mem_pool_transaction(
        &self,
        tx_hash: &H256,
    ) -> Result<Option<packed::L2Transaction>, Error>;

    fn remove_mem_pool_transaction(&self, tx_hash: &H256) -> Result<(), Error>;

    fn insert_mem_pool_transaction_receipt(
        &self,
        tx_hash: &H256,
        tx_receipt: packed::TxReceipt,
    ) -> Result<(), Error>;

    fn get_mem_pool_transaction_receipt(
        &self,
        tx_hash: &H256,
    ) -> Result<Option<packed::TxReceipt>, Error>;

    fn insert_mem_pool_withdrawal(
        &self,
        withdrawal_hash: &H256,
        withdrawal: packed::WithdrawalRequest,
    ) -> Result<(), Error>;

    fn get_mem_pool_withdrawal(
        &self,
        withdrawal_hash: &H256,
    ) -> Result<Option<packed::WithdrawalRequest>, Error>;

    fn remove_mem_pool_withdrawal(&self, withdrawal_hash: &H256) -> Result<(), Error>;

    fn update_mem_pool_block_info(&self, block_info: &packed::BlockInfo) -> Result<(), Error>;

    fn get_mem_pool_block_info(&self) -> Result<Option<packed::BlockInfo>, Error>;

    fn clear_mem_block_state(&self) -> Result<(), Error>;
}

impl MemPoolStore for StoreTransaction {
    fn clear_mem_block_state(&self) -> Result<(), Error> {
        self.clear_block_state(MEMORY_BLOCK_NUMBER)
    }

    fn insert_mem_pool_transaction(
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

    fn get_mem_pool_transaction(
        &self,
        tx_hash: &H256,
    ) -> Result<Option<packed::L2Transaction>, Error> {
        Ok(self
            .get(COLUMN_MEM_POOL_TRANSACTION, tx_hash.as_slice())
            .map(|slice| {
                packed::L2TransactionReader::from_slice_should_be_ok(slice.as_ref()).to_entity()
            }))
    }

    fn remove_mem_pool_transaction(&self, tx_hash: &H256) -> Result<(), Error> {
        self.delete(COLUMN_MEM_POOL_TRANSACTION, tx_hash.as_slice())?;
        self.delete(COLUMN_MEM_POOL_TRANSACTION_RECEIPT, tx_hash.as_slice())?;
        Ok(())
    }

    fn insert_mem_pool_transaction_receipt(
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

    fn get_mem_pool_transaction_receipt(
        &self,
        tx_hash: &H256,
    ) -> Result<Option<packed::TxReceipt>, Error> {
        Ok(self
            .get(COLUMN_MEM_POOL_TRANSACTION_RECEIPT, tx_hash.as_slice())
            .map(|slice| {
                packed::TxReceiptReader::from_slice_should_be_ok(slice.as_ref()).to_entity()
            }))
    }

    fn insert_mem_pool_withdrawal(
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

    fn get_mem_pool_withdrawal(
        &self,
        withdrawal_hash: &H256,
    ) -> Result<Option<packed::WithdrawalRequest>, Error> {
        Ok(self
            .get(COLUMN_MEM_POOL_WITHDRAWAL, withdrawal_hash.as_slice())
            .map(|slice| {
                packed::WithdrawalRequestReader::from_slice_should_be_ok(slice.as_ref()).to_entity()
            }))
    }

    fn remove_mem_pool_withdrawal(&self, withdrawal_hash: &H256) -> Result<(), Error> {
        self.delete(COLUMN_MEM_POOL_WITHDRAWAL, withdrawal_hash.as_slice())?;
        Ok(())
    }

    fn update_mem_pool_block_info(&self, block_info: &packed::BlockInfo) -> Result<(), Error> {
        self.insert_raw(COLUMN_META, META_MEM_POOL_BLOCK_INFO, block_info.as_slice())
    }

    fn get_mem_pool_block_info(&self) -> Result<Option<packed::BlockInfo>, Error> {
        Ok(self
            .get(COLUMN_META, META_MEM_POOL_BLOCK_INFO)
            .map(|slice| {
                packed::BlockInfoReader::from_slice_should_be_ok(slice.as_ref()).to_entity()
            }))
    }
}
