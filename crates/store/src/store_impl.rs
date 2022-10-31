//! Storage implementation

use crate::smt::smt_store::SMTBlockStore;
use crate::state::history::history_state::RWConfig;
use crate::state::BlockStateDB;
use crate::traits::chain_store::ChainStore;
use crate::traits::kv_store::KVStoreRead;
use crate::write_batch::StoreWriteBatch;
use crate::{snapshot::StoreSnapshot, transaction::StoreTransaction};
use anyhow::Result;
use gw_common::error::Error;
use gw_common::smt::Blake2bHasher;

use gw_db::{
    schema::{Col, COLUMNS},
    CfMemStat, DBPinnableSlice, RocksDB,
};
use gw_types::prelude::*;

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

    pub fn begin_transaction(&self) -> StoreTransaction {
        StoreTransaction {
            inner: self.db.transaction(),
        }
    }

    pub fn gather_mem_stats(&self) -> Vec<CfMemStat> {
        self.db.gather_mem_stats()
    }

    pub fn new_write_batch(&self) -> StoreWriteBatch {
        StoreWriteBatch {
            inner: self.db.new_write_batch(),
        }
    }

    pub fn write(&self, write_batch: &StoreWriteBatch) -> Result<(), Error> {
        if let Err(err) = self.db.write(&write_batch.inner) {
            log::error!("Store error: {}", err);
            return Err(Error::Store);
        }
        Ok(())
    }

    pub fn check_state(&self) -> Result<()> {
        // check state tree
        {
            let db = self.begin_transaction();
            let tree = BlockStateDB::from_store(&db, RWConfig::readonly())?;
            tree.check_state()?;
        }

        // check block smt
        {
            let db = self.get_snapshot();
            let tip_number: u64 = db.get_last_valid_tip_block()?.raw().number().unpack();
            let smt = SMTBlockStore::new(db).to_smt()?;
            for number in tip_number.saturating_sub(100)..tip_number {
                let block_hash = self.get_block_hash_by_number(number)?.expect("exist");
                let block = self.get_block(&block_hash)?.expect("exist");
                let key = block.smt_key();
                let proof = smt.merkle_proof(vec![key.into()])?;
                let root =
                    proof.compute_root::<Blake2bHasher>(vec![(key.into(), block.hash().into())])?;
                assert_eq!(&root, smt.root(), "block smt root consistent");
            }
        }
        Ok(())
    }

    pub fn get_snapshot(&self) -> StoreSnapshot {
        StoreSnapshot::new(self.db.get_snapshot())
    }
}

impl ChainStore for Store {}
impl KVStoreRead for Store {
    fn get(&self, col: Col, key: &[u8]) -> Option<Box<[u8]>> {
        self.get(col, key).map(|v| Box::<[u8]>::from(v.as_ref()))
    }
}
