use super::overlay::{OverlaySMTStore, OverlayStore};
use super::wrap_store::WrapStore;
use crate::snapshot::StoreSnapshot;
use crate::transaction::StoreTransaction;
use crate::write_batch::StoreWriteBatch;
use anyhow::{anyhow, Result};
use gw_common::{
    error::Error,
    smt::{Store as SMTStore, H256, SMT},
    state::State,
};
use gw_db::{
    iter::{DBIter, DBIterator, IteratorMode},
    schema::{
        Col, COLUMN_BLOCK, COLUMN_META, COLUMN_SYNC_BLOCK_HEADER_INFO, COLUMN_TRANSACTION,
        COLUMN_TRANSACTION_RECEIPT, META_TIP_BLOCK_HASH_KEY, META_TIP_GLOBAL_STATE_KEY,
    },
    DBPinnableSlice, RocksDB,
};
use gw_generator::traits::CodeStore;
use gw_types::{
    bytes::Bytes,
    core::TxReceipt,
    packed::{self, GlobalState, HeaderInfo, L2Block, L2Transaction, Script},
    prelude::*,
};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;

pub struct Store {
    db: RocksDB,
}

impl<'a> Store {
    pub fn new(db: RocksDB) -> Self {
        Store { db }
    }

    fn get(&'a self, col: Col, key: &[u8]) -> Option<DBPinnableSlice<'a>> {
        self.db
            .get_pinned(col, key)
            .expect("db operation should be ok")
    }

    fn get_iter(&self, col: Col, mode: IteratorMode) -> DBIter {
        self.db.iter(col, mode).expect("db operation should be ok")
    }

    pub fn begin_transaction(&self) -> StoreTransaction {
        StoreTransaction {
            inner: self.db.transaction(),
        }
    }

    pub fn get_snapshot(&self) -> StoreSnapshot {
        StoreSnapshot {
            inner: self.db.get_snapshot(),
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

    /// TODO use RocksDB snapshot
    pub fn new_overlay<S: SMTStore<H256>>(&self) -> Result<OverlayStore<WrapStore<S>>> {
        unimplemented!()
        // let root = self.account_tree.root();
        // let account_count = self
        //     .get_account_count()
        //     .map_err(|err| anyhow!("get amount count error: {:?}", err))?;
        // let store = OverlaySMTStore::new(self.account_tree.store().clone());
        // Ok(OverlayStore::new(
        //     *root,
        //     store,
        //     account_count,
        //     self.scripts.clone(),
        //     self.data_map.clone(),
        // ))
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

    pub fn get_tip_block(&self) -> Result<Option<L2Block>, Error> {
        let tip_block_hash = self.get_tip_block_hash()?;
        self.get_block(&tip_block_hash)
    }

    pub fn get_tip_global_state(&self) -> Result<GlobalState, Error> {
        let slice = self
            .get(COLUMN_META, META_TIP_GLOBAL_STATE_KEY)
            .expect("get tip global state");
        Ok(packed::GlobalStateReader::from_slice_should_be_ok(&slice.as_ref()).to_entity())
    }

    pub fn get_block(&self, block_hash: &H256) -> Result<Option<L2Block>, Error> {
        match self.get(COLUMN_BLOCK, block_hash.as_slice()) {
            Some(slice) => Ok(Some(
                packed::L2BlockReader::from_slice_should_be_ok(&slice.as_ref()).to_entity(),
            )),
            None => Ok(None),
        }
    }

    pub fn get_block_synced_header_info(
        &self,
        block_hash: &H256,
    ) -> Result<Option<HeaderInfo>, Error> {
        match self.get(COLUMN_SYNC_BLOCK_HEADER_INFO, block_hash.as_slice()) {
            Some(slice) => Ok(Some(
                packed::HeaderInfoReader::from_slice_should_be_ok(&slice.as_ref()).to_entity(),
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
