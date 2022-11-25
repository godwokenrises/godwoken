use crate::db::cf_handle;
use crate::schema::Col;
use anyhow::{Context, Result};
use rocksdb::ops::{DeleteCF, GetCF, PutCF};
pub use rocksdb::{DBPinnableSlice, DBVector};
use rocksdb::{
    OptimisticTransaction, OptimisticTransactionDB, OptimisticTransactionSnapshot, ReadOptions,
};
use std::{fmt, sync::Arc};

#[derive(Debug)]
pub struct CommitError;

impl fmt::Display for CommitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "commit error")
    }
}

impl std::error::Error for CommitError {}

pub struct RocksDBTransaction {
    pub(crate) db: Arc<OptimisticTransactionDB>,
    pub(crate) inner: OptimisticTransaction,
}

impl RocksDBTransaction {
    pub fn get(&self, col: Col, key: &[u8]) -> Result<Option<DBVector>> {
        let cf = cf_handle(&self.db, col)?;
        Ok(self.inner.get_cf(cf, key)?)
    }

    pub fn put(&self, col: Col, key: &[u8], value: &[u8]) -> Result<()> {
        let cf = cf_handle(&self.db, col)?;
        Ok(self.inner.put_cf(cf, key, value)?)
    }

    pub fn delete(&self, col: Col, key: &[u8]) -> Result<()> {
        let cf = cf_handle(&self.db, col)?;
        Ok(self.inner.delete_cf(cf, key)?)
    }

    pub fn get_for_update<'a>(
        &self,
        col: Col,
        key: &[u8],
        snapshot: &RocksDBTransactionSnapshot<'a>,
    ) -> Result<Option<DBVector>> {
        let cf = cf_handle(&self.db, col)?;
        let mut opts = ReadOptions::default();
        opts.set_snapshot(&snapshot.inner);
        Ok(self.inner.get_for_update_cf_opt(cf, key, &opts, true)?)
    }

    pub fn commit(&self) -> Result<()> {
        self.inner.commit().context(CommitError)
    }

    pub fn rollback(&self) -> Result<()> {
        Ok(self.inner.rollback()?)
    }

    pub fn get_snapshot(&self) -> RocksDBTransactionSnapshot<'_> {
        RocksDBTransactionSnapshot {
            db: Arc::clone(&self.db),
            inner: self.inner.snapshot(),
        }
    }

    pub fn set_savepoint(&self) {
        self.inner.set_savepoint()
    }

    pub fn rollback_to_savepoint(&self) -> Result<()> {
        Ok(self.inner.rollback_to_savepoint()?)
    }
}

pub struct RocksDBTransactionSnapshot<'a> {
    pub(crate) db: Arc<OptimisticTransactionDB>,
    pub(crate) inner: OptimisticTransactionSnapshot<'a>,
}

impl<'a> RocksDBTransactionSnapshot<'a> {
    pub fn get(&self, col: Col, key: &[u8]) -> Result<Option<DBVector>> {
        let cf = cf_handle(&self.db, col)?;
        Ok(self.inner.get_cf(cf, key)?)
    }
}
