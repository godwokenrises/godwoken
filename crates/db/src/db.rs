use crate::memory_stats::TrackRocksDBMemory;
use crate::schema::Col;
use crate::snapshot::RocksDBSnapshot;
use crate::transaction::RocksDBTransaction;
use crate::write_batch::RocksDBWriteBatch;
use crate::CfMemStat;
use anyhow::{bail, Context, Result};
use gw_config::StoreConfig;
use rocksdb::ops::{
    CreateCF, DropCF, GetColumnFamilys, GetPinned, GetPinnedCF, IterateCF, OpenCF, Put, SetOptions,
    WriteOps,
};
use rocksdb::{
    ffi, ColumnFamily, ColumnFamilyDescriptor, DBPinnableSlice, FullOptions, IteratorMode,
    OptimisticTransactionDB, OptimisticTransactionOptions, Options, WriteBatch, WriteOptions,
};
use std::sync::Arc;

/// RocksDB wrapper base on OptimisticTransactionDB
///
/// https://github.com/facebook/rocksdb/wiki/Transactions#optimistictransactiondb
#[derive(Clone)]
pub struct RocksDB {
    pub(crate) inner: Arc<OptimisticTransactionDB>,
}

impl RocksDB {
    pub(crate) fn open_with_check(config: &StoreConfig, columns: u32) -> Result<Self> {
        let cf_names: Vec<_> = (0..columns).map(|c| c.to_string()).collect();

        let (mut opts, cf_descriptors) = if let Some(ref file) = config.options_file {
            let mut full_opts = FullOptions::load_from_file(file, config.cache_size, false)
                .context("load options from file")?;
            let cf_names_str: Vec<&str> = cf_names.iter().map(|s| s.as_str()).collect();
            full_opts
                .complete_column_families(&cf_names_str, false)
                .context("complete column families")?;
            let FullOptions {
                db_opts,
                cf_descriptors,
            } = full_opts;
            (db_opts, cf_descriptors)
        } else {
            let opts = Options::default();
            let cf_descriptors: Vec<_> = cf_names
                .iter()
                .map(|c| ColumnFamilyDescriptor::new(c, Options::default()))
                .collect();
            (opts, cf_descriptors)
        };

        opts.create_if_missing(false);
        opts.create_missing_column_families(true);

        let db = OptimisticTransactionDB::open_cf_descriptors(
            &opts,
            &config.path,
            cf_descriptors.clone(),
        )
        .or_else(|err| {
            let err_str = err.as_ref();
            if err_str.starts_with("Invalid argument:")
                && err_str.ends_with("does not exist (create_if_missing is false)")
            {
                opts.create_if_missing(true);
                let db = OptimisticTransactionDB::open_cf_descriptors(
                    &opts,
                    &config.path,
                    cf_descriptors.clone(),
                )
                .context("open a new created database")?;
                Ok(db)
            } else if err.as_ref().starts_with("Corruption:") {
                eprintln!("Repairing the rocksdb since {} ...", err);
                let mut repair_opts = Options::default();
                repair_opts.create_if_missing(false);
                repair_opts.create_missing_column_families(false);
                OptimisticTransactionDB::repair(repair_opts, &config.path)
                    .context("repair database")?;
                eprintln!("Opening the repaired rocksdb ...");
                OptimisticTransactionDB::open_cf_descriptors(
                    &opts,
                    &config.path,
                    cf_descriptors.clone(),
                )
                .context("open repaired database")
            } else {
                bail!("failed to open database: {}", err);
            }
        })?;

        if !config.options.is_empty() {
            let rocksdb_options: Vec<(&str, &str)> = config
                .options
                .iter()
                .map(|(k, v)| (k.as_str(), v.as_str()))
                .collect();
            db.set_options(&rocksdb_options)
                .context("failed to set database option")?;
        }

        Ok(RocksDB {
            inner: Arc::new(db),
        })
    }

    pub fn open(config: &StoreConfig, columns: u32) -> Self {
        Self::open_with_check(config, columns).unwrap_or_else(|err| panic!("{}", err))
    }

    pub fn open_tmp(columns: u32) -> Self {
        let tmp_dir = tempfile::Builder::new().tempdir().unwrap();
        let config = StoreConfig {
            path: tmp_dir.path().to_path_buf(),
            ..Default::default()
        };
        Self::open_with_check(&config, columns).unwrap_or_else(|err| panic!("{}", err))
    }

    pub fn get_pinned(&self, col: Col, key: &[u8]) -> Result<Option<DBPinnableSlice>> {
        let cf = cf_handle(&self.inner, col)?;
        Ok(self.inner.get_pinned_cf(cf, &key)?)
    }

    pub fn get_pinned_default(&self, key: &[u8]) -> Result<Option<DBPinnableSlice>> {
        Ok(self.inner.get_pinned(&key)?)
    }

    pub fn put_default<K, V>(&self, key: K, value: V) -> Result<()>
    where
        K: AsRef<[u8]>,
        V: AsRef<[u8]>,
    {
        Ok(self.inner.put(key, value)?)
    }

    pub fn traverse<F>(&self, col: Col, mut callback: F) -> Result<()>
    where
        F: FnMut(&[u8], &[u8]) -> Result<()>,
    {
        let cf = cf_handle(&self.inner, col)?;
        let iter = self.inner.full_iterator_cf(cf, IteratorMode::Start)?;
        for (key, val) in iter {
            callback(&key, &val)?;
        }
        Ok(())
    }

    /// Set a snapshot at start of transaction by setting set_snapshot=true
    pub fn transaction(&self) -> RocksDBTransaction {
        let write_options = WriteOptions::default();
        let mut transaction_options = OptimisticTransactionOptions::new();
        transaction_options.set_snapshot(true);

        RocksDBTransaction {
            db: Arc::clone(&self.inner),
            inner: self.inner.transaction(&write_options, &transaction_options),
        }
    }

    pub fn new_write_batch(&self) -> RocksDBWriteBatch {
        RocksDBWriteBatch {
            db: Arc::clone(&self.inner),
            inner: WriteBatch::default(),
        }
    }

    pub fn write(&self, batch: &RocksDBWriteBatch) -> Result<()> {
        Ok(self.inner.write(&batch.inner)?)
    }

    pub fn get_snapshot(&self) -> RocksDBSnapshot {
        unsafe {
            let snapshot = ffi::rocksdb_create_snapshot(self.inner.base_db_ptr());
            RocksDBSnapshot::new(&self.inner, snapshot)
        }
    }

    pub fn inner(&self) -> Arc<OptimisticTransactionDB> {
        Arc::clone(&self.inner)
    }

    pub fn create_cf(&mut self, col: Col) -> Result<()> {
        let inner = Arc::get_mut(&mut self.inner).context("create_cf get_mut")?;
        let opts = Options::default();
        Ok(inner.create_cf(&col.to_string(), &opts)?)
    }

    pub fn drop_cf(&mut self, col: Col) -> Result<()> {
        let inner = Arc::get_mut(&mut self.inner).context("drop_cf get_mut")?;
        Ok(inner.drop_cf(&col.to_string())?)
    }

    pub fn gather_mem_stats(&self) -> Vec<CfMemStat> {
        self.inner.gather_memory_stats()
    }
}

#[inline]
pub(crate) fn cf_handle(db: &OptimisticTransactionDB, col: Col) -> Result<&ColumnFamily> {
    db.cf_handle(&col.to_string())
        .with_context(|| format!("column family {col} not found"))
}

#[cfg(test)]
mod tests {
    use super::{Result, RocksDB, StoreConfig};
    use std::collections::HashMap;

    fn setup_db(prefix: &str, columns: u32) -> RocksDB {
        setup_db_with_check(prefix, columns).unwrap()
    }

    fn setup_db_with_check(prefix: &str, columns: u32) -> Result<RocksDB> {
        let tmp_dir = tempfile::Builder::new().prefix(prefix).tempdir().unwrap();
        let config = StoreConfig {
            path: tmp_dir.as_ref().to_path_buf(),
            ..Default::default()
        };

        RocksDB::open_with_check(&config, columns)
    }

    #[test]
    fn test_set_rocksdb_options() {
        let tmp_dir = tempfile::Builder::new()
            .prefix("test_set_rocksdb_options")
            .tempdir()
            .unwrap();
        let config = StoreConfig {
            path: tmp_dir.as_ref().to_path_buf(),
            options: {
                let mut opts = HashMap::new();
                opts.insert("disable_auto_compactions".to_owned(), "true".to_owned());
                opts
            },
            options_file: None,
            cache_size: None,
        };
        RocksDB::open(&config, 2); // no panic
    }

    #[test]
    fn test_set_rocksdb_options_empty() {
        let tmp_dir = tempfile::Builder::new()
            .prefix("test_set_rocksdb_options_empty")
            .tempdir()
            .unwrap();
        let config = StoreConfig {
            path: tmp_dir.as_ref().to_path_buf(),
            options: HashMap::new(),
            options_file: None,
            cache_size: None,
        };
        RocksDB::open(&config, 2); // no panic
    }

    #[test]
    #[should_panic]
    fn test_panic_on_invalid_rocksdb_options() {
        let tmp_dir = tempfile::Builder::new()
            .prefix("test_panic_on_invalid_rocksdb_options")
            .tempdir()
            .unwrap();
        let config = StoreConfig {
            path: tmp_dir.as_ref().to_path_buf(),
            options: {
                let mut opts = HashMap::new();
                opts.insert("letsrock".to_owned(), "true".to_owned());
                opts
            },
            options_file: None,
            cache_size: None,
        };
        RocksDB::open(&config, 2); // panic
    }

    #[test]
    fn write_and_read() {
        let db = setup_db("write_and_read", 2);

        let txn = db.transaction();
        txn.put(0, &[0, 0], &[0, 0, 0]).unwrap();
        txn.put(1, &[1, 1], &[1, 1, 1]).unwrap();
        txn.put(1, &[2], &[1, 1, 1]).unwrap();
        txn.delete(1, &[2]).unwrap();
        txn.commit().unwrap();

        assert!(vec![0u8, 0, 0].as_slice() == db.get_pinned(0, &[0, 0]).unwrap().unwrap().as_ref());
        assert!(db.get_pinned(0, &[1, 1]).unwrap().is_none());

        assert!(db.get_pinned(1, &[0, 0]).unwrap().is_none());
        assert!(vec![1u8, 1, 1].as_slice() == db.get_pinned(1, &[1, 1]).unwrap().unwrap().as_ref());

        assert!(db.get_pinned(1, &[2]).unwrap().is_none());

        let mut r = HashMap::new();
        let callback = |k: &[u8], v: &[u8]| -> Result<()> {
            r.insert(k.to_vec(), v.to_vec());
            Ok(())
        };
        db.traverse(1, callback).unwrap();
        assert!(r.len() == 1);
        assert_eq!(r.get(&vec![1, 1]), Some(&vec![1, 1, 1]));
    }

    #[test]
    fn snapshot_isolation() {
        let db = setup_db("snapshot_isolation", 2);
        let snapshot = db.get_snapshot();
        let txn = db.transaction();
        txn.put(0, &[0, 0], &[5, 4, 3, 2]).unwrap();
        txn.put(1, &[1, 1], &[1, 2, 3, 4, 5]).unwrap();
        txn.commit().unwrap();

        assert!(snapshot.get_pinned(0, &[0, 0]).unwrap().is_none());
        assert!(snapshot.get_pinned(1, &[1, 1]).unwrap().is_none());
        let snapshot = db.get_snapshot();
        assert_eq!(
            snapshot.get_pinned(0, &[0, 0]).unwrap().unwrap().as_ref(),
            &[5, 4, 3, 2]
        );
        assert_eq!(
            snapshot.get_pinned(1, &[1, 1]).unwrap().unwrap().as_ref(),
            &[1, 2, 3, 4, 5]
        );
    }

    #[test]
    fn write_and_partial_read() {
        let db = setup_db("write_and_partial_read", 2);

        let txn = db.transaction();
        txn.put(0, &[0, 0], &[5, 4, 3, 2]).unwrap();
        txn.put(1, &[1, 1], &[1, 2, 3, 4, 5]).unwrap();
        txn.commit().unwrap();

        let ret = db.get_pinned(1, &[1, 1]).unwrap().unwrap();

        assert!(vec![2u8, 3, 4].as_slice() == &ret.as_ref()[1..4]);
        assert!(db.get_pinned(1, &[0, 0]).unwrap().is_none());

        let ret = db.get_pinned(0, &[0, 0]).unwrap().unwrap();

        assert!(vec![4u8, 3, 2].as_slice() == &ret.as_ref()[1..4]);
    }
}
