//! Storage implementation

use crate::{state_db::StateDBTransaction, write_batch::StoreWriteBatch};
use crate::{state_db::StateDBVersion, transaction::StoreTransaction};
use anyhow::Result;
use gw_common::{error::Error, smt::H256};
use gw_db::{
    schema::{
        Col, COLUMNS, COLUMN_BLOCK, COLUMN_BLOCK_GLOBAL_STATE, COLUMN_L2BLOCK_COMMITTED_INFO,
        COLUMN_META, COLUMN_TRANSACTION, COLUMN_TRANSACTION_RECEIPT, META_CHAIN_ID_KEY,
        META_TIP_BLOCK_HASH_KEY,
    },
    DBPinnableSlice, RocksDB,
};
use gw_types::{
    packed::{self, GlobalState, L2Block, L2Transaction},
    prelude::*,
};
use std::rc::Rc;

#[derive(Clone)]
pub struct Store {
    db: RocksDB,
}

impl<'a> Store {
    pub fn new(db: RocksDB) -> Self {
        Store { db }
    }

    pub fn open_tmp() -> Result<Self> {
        let db = RocksDB::open_tmp(COLUMNS);
        Ok(Self::new(db))
    }

    fn get(&'a self, col: Col, key: &[u8]) -> Option<DBPinnableSlice<'a>> {
        self.db
            .get_pinned(col, key)
            .expect("db operation should be ok")
    }

    // fn get_iter(&self, col: Col, mode: IteratorMode) -> DBIter {
    //     self.db.iter(col, mode).expect("db operation should be ok")
    // }

    pub fn begin_transaction(&self) -> StoreTransaction {
        StoreTransaction {
            inner: Rc::new(self.db.transaction()),
        }
    }

    pub fn new_write_batch(&self) -> StoreWriteBatch {
        StoreWriteBatch {
            inner: self.db.new_write_batch(),
        }
    }

    pub fn write(&self, write_batch: &StoreWriteBatch) -> Result<(), Error> {
        if let Err(err) = self.db.write(&write_batch.inner) {
            eprintln!("Store error: {}", err);
            return Err(Error::Store);
        }
        Ok(())
    }

    pub fn has_genesis(&self) -> Result<bool> {
        let db = self.begin_transaction();
        Ok(db.get_block_hash_by_number(0)?.is_some())
    }

    /// Return state at version
    pub fn state_at(&self, version: StateDBVersion) -> Result<StateDBTransaction> {
        let db = self.begin_transaction();
        let state_db = StateDBTransaction::from_version(db, version)?;
        Ok(state_db)
    }

    pub fn get_chain_id(&self) -> Result<H256, Error> {
        let slice = self
            .get(COLUMN_META, META_CHAIN_ID_KEY)
            .expect("must has chain_id");
        debug_assert_eq!(slice.len(), 32);
        let mut chain_id = [0u8; 32];
        chain_id.copy_from_slice(&slice);
        Ok(chain_id.into())
    }

    pub fn get_tip_block_hash(&self) -> Result<H256, Error> {
        let slice = self
            .get(COLUMN_META, META_TIP_BLOCK_HASH_KEY)
            .expect("get tip block hash");
        Ok(
            packed::Byte32Reader::from_slice_should_be_ok(&slice.as_ref())
                .to_entity()
                .unpack(),
        )
    }

    pub fn get_tip_block(&self) -> Result<L2Block, Error> {
        let tip_block_hash = self.get_tip_block_hash()?;
        Ok(self.get_block(&tip_block_hash)?.expect("get tip block"))
    }

    pub fn get_block_post_global_state(
        &self,
        block_hash: &H256,
    ) -> Result<Option<GlobalState>, Error> {
        match self.get(COLUMN_BLOCK_GLOBAL_STATE, block_hash.as_slice()) {
            Some(slice) => Ok(Some(
                packed::GlobalStateReader::from_slice_should_be_ok(&slice.as_ref()).to_entity(),
            )),
            None => Ok(None),
        }
    }

    pub fn get_block(&self, block_hash: &H256) -> Result<Option<L2Block>, Error> {
        match self.get(COLUMN_BLOCK, block_hash.as_slice()) {
            Some(slice) => Ok(Some(
                packed::L2BlockReader::from_slice_should_be_ok(&slice.as_ref()).to_entity(),
            )),
            None => Ok(None),
        }
    }

    pub fn get_l2block_committed_info(
        &self,
        block_hash: &H256,
    ) -> Result<Option<packed::L2BlockCommittedInfo>, Error> {
        match self.get(COLUMN_L2BLOCK_COMMITTED_INFO, block_hash.as_slice()) {
            Some(slice) => Ok(Some(
                packed::L2BlockCommittedInfoReader::from_slice_should_be_ok(&slice.as_ref())
                    .to_entity(),
            )),
            None => Ok(None),
        }
    }

    pub fn get_transaction(&self, tx_hash: &H256) -> Result<Option<L2Transaction>, Error> {
        match self.get(COLUMN_TRANSACTION, tx_hash.as_slice()) {
            Some(slice) => Ok(Some(
                packed::L2TransactionReader::from_slice_should_be_ok(&slice.as_ref()).to_entity(),
            )),
            None => Ok(None),
        }
    }

    pub fn get_transaction_receipt(
        &self,
        tx_hash: &H256,
    ) -> Result<Option<packed::TxReceipt>, Error> {
        match self.get(COLUMN_TRANSACTION_RECEIPT, tx_hash.as_slice()) {
            Some(slice) => Ok(Some(
                packed::TxReceiptReader::from_slice_should_be_ok(&slice.as_ref()).to_entity(),
            )),
            None => Ok(None),
        }
    }
}
