//! TODO(doc): @quake
use crate::db::cf_handle;
use crate::read_only_db::ReadOnlyDB;
use crate::schema::Col;
use crate::{RocksDB, RocksDBSnapshot, RocksDBTransaction, RocksDBTransactionSnapshot};
use anyhow::{Context, Result};
use rocksdb::ops::GetColumnFamilys;
use rocksdb::{ops::IterateCF, ReadOptions};
pub use rocksdb::{DBIterator as DBIter, Direction, IteratorMode};

/// TODO(doc): @quake
pub type DBIterItem = (Box<[u8]>, Box<[u8]>);

/// TODO(doc): @quake
pub trait DBIterator {
    /// TODO(doc): @quake
    fn iter(&self, col: Col, mode: IteratorMode) -> Result<DBIter> {
        let opts = ReadOptions::default();
        self.iter_opt(col, mode, &opts)
    }

    /// TODO(doc): @quake
    fn iter_opt(&self, col: Col, mode: IteratorMode, readopts: &ReadOptions) -> Result<DBIter>;
}

impl DBIterator for RocksDB {
    fn iter_opt(&self, col: Col, mode: IteratorMode, readopts: &ReadOptions) -> Result<DBIter> {
        let cf = cf_handle(&self.inner, col)?;
        Ok(self.inner.iterator_cf_opt(cf, mode, readopts)?)
    }
}

impl DBIterator for RocksDBTransaction {
    fn iter_opt(&self, col: Col, mode: IteratorMode, readopts: &ReadOptions) -> Result<DBIter> {
        let cf = cf_handle(&self.db, col)?;
        Ok(self.inner.iterator_cf_opt(cf, mode, readopts)?)
    }
}

impl<'a> DBIterator for RocksDBTransactionSnapshot<'a> {
    fn iter_opt(&self, col: Col, mode: IteratorMode, readopts: &ReadOptions) -> Result<DBIter> {
        let cf = cf_handle(&self.db, col)?;
        Ok(self.inner.iterator_cf_opt(cf, mode, readopts)?)
    }
}

impl DBIterator for RocksDBSnapshot {
    fn iter_opt(&self, col: Col, mode: IteratorMode, readopts: &ReadOptions) -> Result<DBIter> {
        let cf = cf_handle(&self.db, col)?;
        Ok(self.iterator_cf_opt(cf, mode, readopts)?)
    }
}

impl DBIterator for ReadOnlyDB {
    fn iter_opt(&self, col: Col, mode: IteratorMode, readopts: &ReadOptions) -> Result<DBIter> {
        let cf = self
            .inner
            .cf_handle(&col.to_string())
            .with_context(|| format!("column {col} not found"))?;
        Ok(self.inner.iterator_cf_opt(cf, mode, readopts)?)
    }
}
