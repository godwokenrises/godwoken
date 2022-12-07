//! Storage implementation

use std::pin::Pin;
use std::sync::Arc;

use anyhow::Result;
use autorocks::autorocks_sys::rocksdb::{
    PinnableSlice, TransactionDBWriteOptimizations, TransactionOptions, WriteOptions,
};
use autorocks::moveit::moveit;
use autorocks::{DbOptions, TransactionDb, WriteBatch};
use gw_common::smt::Blake2bHasher;
use gw_config::StoreConfig;
use gw_types::prelude::*;
use serde::Serialize;
use tempfile::TempDir;

use crate::schema::{Col, COLUMNS};
use crate::smt::smt_store::SMTBlockStore;
use crate::state::{history::history_state::RWConfig, BlockStateDB};
use crate::traits::{chain_store::ChainStore, kv_store::KVStoreRead};
use crate::{snapshot::StoreSnapshot, transaction::StoreTransaction};

#[derive(Clone)]
pub struct Store {
    db: TransactionDb,
    _temp_dir: Option<Arc<TempDir>>,
}

impl<'a> Store {
    pub fn open(config: &StoreConfig, columns: usize) -> Result<Self> {
        let mut opts = DbOptions::new(&config.path, columns);
        if let Some(ref opts_file) = config.options_file {
            opts.load_options_from_file(opts_file)?;
        }
        opts.create_if_missing(true);
        opts.create_missing_column_families(true);
        let db = opts.open()?;
        // TODO: repair.
        Ok(Self::new(db))
    }

    pub fn new(db: TransactionDb) -> Self {
        Store {
            db,
            _temp_dir: None,
        }
    }

    pub fn open_tmp() -> Result<Self> {
        let dir = tempfile::tempdir()?;
        Ok(Self {
            db: DbOptions::new(dir.path(), COLUMNS)
                .create_if_missing(true)
                .create_missing_column_families(true)
                .open()?,
            _temp_dir: Some(dir.into()),
        })
    }

    fn get<'b>(
        &'a self,
        col: Col,
        key: &[u8],
        buf: Pin<&'b mut PinnableSlice>,
    ) -> Option<&'b [u8]> {
        self.db
            .get(col, key, buf)
            .expect("db operation should be ok")
    }

    pub fn begin_transaction(&self) -> StoreTransaction {
        StoreTransaction {
            inner: self.db.begin_transaction(),
        }
    }

    /// Begin transaction but disable concurrency control.
    ///
    /// This should be faster than a normal transaction when you know that there
    /// won't be any conflicts.
    pub fn begin_transaction_skip_concurrency_control(&self) -> StoreTransaction {
        moveit! {
            let write_options = WriteOptions::new();
            let mut transaction_options = TransactionOptions::new();
        }
        transaction_options.as_mut().skip_concurrency_control = true;
        StoreTransaction {
            inner: self
                .db
                .begin_transaction_with_options(&write_options, &transaction_options),
        }
    }

    pub fn gather_mem_stats(&self) -> Vec<CfMemStat> {
        let last_col = self.as_inner().default_col();
        let mut result = Vec::with_capacity((last_col + 1) * 6);

        for c in 0..=last_col {
            for p in [
                "rocksdb.estimate-table-readers-mem",
                "rocksdb.size-all-mem-tables",
                "rocksdb.cur-size-all-mem-tables",
                "rocksdb.block-cache-capacity",
                "rocksdb.block-cache-usage",
                "rocksdb.block-cache-pinned-usage",
            ] {
                result.push(CfMemStat {
                    name: c,
                    // Skip rocksdb.
                    type_: &p[8..],
                    value: self.as_inner().get_int_property(c, p),
                })
            }
        }
        result
    }

    /// Transactional range delete is not supported. If there are range deletes
    /// in the write_batch, must use this.
    pub fn write_skip_concurrency_control(&self, write_batch: &mut WriteBatch) -> Result<()> {
        moveit! {
            let options = WriteOptions::new();
            let mut optimizations = TransactionDBWriteOptimizations::new();
        }
        optimizations.skip_concurrency_control = true;
        self.db
            .write_with_options(&options, &optimizations, write_batch)?;
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
        StoreSnapshot::new(self.db.snapshot())
    }

    pub fn as_inner(&self) -> &TransactionDb {
        &self.db
    }

    pub fn into_inner(self) -> TransactionDb {
        self.db
    }
}

impl ChainStore for Store {}

impl KVStoreRead for Store {
    fn get(&self, col: Col, key: &[u8]) -> Option<Box<[u8]>> {
        moveit! {
            let mut buf = PinnableSlice::new();
        }
        self.get(col, key, buf.as_mut()).map(Into::into)
    }
}

#[derive(Serialize)]
pub struct CfMemStat {
    // Column name.
    name: usize,
    type_: &'static str,
    value: Option<u64>,
}
