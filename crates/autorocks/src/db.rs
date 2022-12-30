use std::{mem::MaybeUninit, os::unix::prelude::OsStrExt, path::Path, pin::Pin, sync::Arc};

use autorocks_sys::{
    new_transaction_db_options, new_write_batch,
    rocksdb::{
        CompressionType, PinnableSlice, ReadOptions, Slice, TransactionDBOptions,
        TransactionDBWriteOptimizations, TransactionOptions, WriteOptions,
    },
    DbOptionsWrapper, ReadOnlyDbWrapper, TransactionDBWrapper, TransactionWrapper,
};
use moveit::{moveit, Emplace, New, Slot};

use crate::{
    into_result, slice::PinnedSlice, DbIterator, Direction, Result, RocksDBStatusError, Snapshot,
    Transaction, WriteBatch,
};

pub struct DbOptions {
    inner: Pin<Box<DbOptionsWrapper>>,
}

impl DbOptions {
    pub fn new(path: &Path, columns: usize) -> Self {
        Self {
            inner: Box::emplace(DbOptionsWrapper::new2(
                path.as_os_str().as_bytes().into(),
                columns,
            )),
        }
    }

    /// Note that this resets all options and column families.
    ///
    /// If cache_capacity > 0, will use a shared cache.
    pub fn load_options_from_file(
        &mut self,
        options_file: &Path,
        cache_capacity: usize,
    ) -> Result<()> {
        moveit! {
            let status = self.inner.as_mut().load(options_file.as_os_str().as_bytes().into(), cache_capacity);
        }
        into_result(&status)
    }

    pub fn create_if_missing(&mut self, val: bool) -> &mut Self {
        self.inner.as_mut().set_create_if_missing(val);
        self
    }

    pub fn create_missing_column_families(&mut self, val: bool) -> &mut Self {
        self.inner.as_mut().set_create_missing_column_families(val);
        self
    }

    /// The corresponding feature must be enabled for this to actually work.
    pub fn compression(&mut self, c: CompressionType) -> &mut Self {
        self.inner.as_mut().set_compression(c);
        self
    }

    pub fn repair(&self) -> Result<()> {
        moveit! {
            let status = self.inner.repair();
        }
        into_result(&status)
    }

    pub fn open_read_only(&self) -> Result<ReadOnlyDb> {
        ReadOnlyDb::open(&self.inner)
    }

    pub fn open(&self) -> Result<TransactionDb> {
        moveit! {
            let txn_db_options = new_transaction_db_options();
        }
        TransactionDb::open(&self.inner, &txn_db_options)
    }
}

#[derive(Clone)]
pub struct TransactionDb {
    inner: Arc<TransactionDBWrapper>,
}

impl TransactionDb {
    fn open(
        options: &DbOptionsWrapper,
        txn_db_options: &TransactionDBOptions,
    ) -> Result<TransactionDb> {
        let db = Arc::emplace(TransactionDBWrapper::new());
        let mut db = Pin::into_inner(db);
        let db_mut = Arc::get_mut(&mut db).unwrap();
        moveit! {
            let status = Pin::new(db_mut).open(options, txn_db_options);
        }
        into_result(&status)?;
        Ok(TransactionDb { inner: db })
    }

    pub fn put(&self, col: usize, key: &[u8], value: &[u8]) -> Result<()> {
        moveit! {
            let options = WriteOptions::new();
        }
        self.put_with_options(&options, col, key, value)
    }

    pub fn default_col(&self) -> usize {
        self.inner.default_col()
    }

    /// Delete all keys in a column family.
    ///
    /// Internally, this drops and re-creates the column family.
    ///
    /// This only works when self is the sole instance of the db.
    pub fn clear_cf(&mut self, col: usize) -> Result<()> {
        let inner = Arc::get_mut(&mut self.inner).ok_or_else(|| RocksDBStatusError {
            msg: "Arc::get_mut failed".into(),
            sub_code: autorocks_sys::rocksdb::Status_SubCode::kNone,
            code: autorocks_sys::rocksdb::Status_Code::kBusy,
        })?;
        moveit! {
            let status = Pin::new(inner).clear_cf(col);
        }
        into_result(&status)
    }

    /// This only works when self is the sole instance of the db.
    pub fn drop_cf(&mut self, col: usize) -> Result<()> {
        let inner = Arc::get_mut(&mut self.inner).ok_or_else(|| RocksDBStatusError {
            msg: "Arc::get_mut failed".into(),
            sub_code: autorocks_sys::rocksdb::Status_SubCode::kNone,
            code: autorocks_sys::rocksdb::Status_Code::kBusy,
        })?;
        moveit! {
            let status = Pin::new(inner).drop_cf(col);
        }
        into_result(&status)
    }

    pub fn put_with_options(
        &self,
        options: &WriteOptions,
        col: usize,
        key: &[u8],
        value: &[u8],
    ) -> Result<()> {
        let cf = self.inner.get_cf(col);
        assert!(!cf.is_null());
        moveit! {
            let status = unsafe { self.inner.put(options, cf, &key.into(), &value.into()) };
        }
        into_result(&status)
    }

    pub fn delete_with_options(
        &self,
        options: &WriteOptions,
        col: usize,
        key: &[u8],
    ) -> Result<()> {
        let cf = self.inner.get_cf(col);
        assert!(!cf.is_null());
        moveit! {
            let status = unsafe { self.inner.del(options, cf, &key.into()) };
        }
        into_result(&status)
    }

    pub fn delete(&self, col: usize, key: &[u8]) -> Result<()> {
        moveit! {
            let options = WriteOptions::new();
        }
        self.delete_with_options(&options, col, key)
    }

    pub fn get<'a>(
        &'a self,
        col: usize,
        key: &[u8],
        slot: Slot<'a, PinnableSlice>,
    ) -> Result<Option<PinnedSlice<'a>>> {
        moveit! {
            let options = ReadOptions::new();
        }
        self.get_with_options(&options, col, key, slot)
    }

    pub fn get_with_options<'a>(
        &'a self,
        options: &ReadOptions,
        col: usize,
        key: &[u8],
        slot: Slot<'a, PinnableSlice>,
    ) -> Result<Option<PinnedSlice<'a>>> {
        let cf = self.inner.get_cf(col);
        assert!(!cf.is_null());
        let mut slice = slot.emplace(PinnableSlice::new());
        let slice_ptr = unsafe { slice.as_mut().get_unchecked_mut() };
        moveit! {
            let status = unsafe { self.inner.get(options, cf, &key.into(), slice_ptr) };
        }
        if status.IsNotFound() {
            return Ok(None);
        }
        into_result(&status)?;
        Ok(Some(PinnedSlice::new(slice)))
    }

    pub fn get_int_property(&self, col: usize, property: &str) -> Option<u64> {
        let cf = self.inner.get_cf(col);
        assert!(!cf.is_null());
        let mut val = 0;
        let got = unsafe {
            self.inner
                .get_int_property(cf, &property.as_bytes().into(), &mut val)
        };
        got.then_some(val)
    }

    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            inner: self.inner.get_snapshot(),
            db: self.clone(),
        }
    }

    /// Begin transaction with default options (but set_snapshot = true).
    pub fn begin_transaction(&self) -> Transaction {
        moveit! {
            let write_options = WriteOptions::new();
            let mut transaction_options = TransactionOptions::new();
        }
        transaction_options.set_snapshot = true;
        self.begin_transaction_with_options(&write_options, &transaction_options)
    }

    pub fn begin_transaction_with_options(
        &self,
        write_options: &WriteOptions,
        transaction_options: &TransactionOptions,
    ) -> Transaction {
        let mut tx: MaybeUninit<TransactionWrapper> = MaybeUninit::uninit();
        unsafe {
            self.inner
                .begin(write_options, transaction_options)
                .new(Pin::new(&mut tx))
        };
        Transaction {
            inner: unsafe { tx.assume_init() },
            db: self.clone(),
        }
    }

    pub fn iter(&self, col: usize, dir: Direction) -> DbIterator<&'_ Self> {
        moveit! {
            let options = ReadOptions::new();
        }
        self.iter_with_options(&options, col, dir)
    }

    pub fn iter_with_options<'a>(
        &'a self,
        options: &ReadOptions,
        col: usize,
        dir: Direction,
    ) -> DbIterator<&'a Self> {
        let cf = self.inner.get_cf(col);
        assert!(!cf.is_null());
        unsafe { DbIterator::new(self.as_inner().iter(options, cf), dir) }
    }

    pub fn new_write_batch(&self) -> WriteBatch {
        WriteBatch {
            inner: new_write_batch(),
            db: self.clone(),
        }
    }

    pub fn write_with_options(
        &self,
        options: &WriteOptions,
        optimizations: &TransactionDBWriteOptimizations,
        updates: &mut WriteBatch,
    ) -> Result<()> {
        moveit! {
            let status = unsafe {
                self.inner.write(options, optimizations, updates.as_inner_mut().get_unchecked_mut())
            };
        }
        into_result(&status)
    }

    pub fn write(&self, updates: &mut WriteBatch) -> Result<()> {
        moveit! {
            let options = WriteOptions::new();
            let optimizations = TransactionDBWriteOptimizations::new();
        }
        self.write_with_options(&options, &optimizations, updates)
    }

    pub fn set_options<K: AsRef<[u8]>, V: AsRef<[u8]>>(
        &self,
        col: usize,
        options: impl IntoIterator<Item = (K, V)>,
    ) -> Result<()> {
        let cf = self.inner.get_cf(col);
        assert!(!cf.is_null());
        let (keys, values): (Vec<Slice>, Vec<Slice>) = options
            .into_iter()
            .map(|(k, v)| (k.as_ref().into(), v.as_ref().into()))
            .unzip();
        moveit! {
            let status = unsafe { self.inner.set_options(cf, keys.as_ptr(), values.as_ptr(), keys.len()) };
        }
        into_result(&status)
    }

    pub fn set_db_options<K: AsRef<[u8]>, V: AsRef<[u8]>>(
        &self,
        options: impl IntoIterator<Item = (K, V)>,
    ) -> Result<()> {
        let (keys, values): (Vec<Slice>, Vec<Slice>) = options
            .into_iter()
            .map(|(k, v)| (k.as_ref().into(), v.as_ref().into()))
            .unzip();
        moveit! {
            let status = unsafe { self.inner.set_db_options(keys.as_ptr(), values.as_ptr(), keys.len()) };
        }
        into_result(&status)
    }

    pub fn as_inner(&self) -> &TransactionDBWrapper {
        &self.inner
    }
}

#[derive(Clone)]
pub struct ReadOnlyDb {
    inner: Arc<ReadOnlyDbWrapper>,
}

impl ReadOnlyDb {
    fn open(options: &DbOptionsWrapper) -> Result<ReadOnlyDb> {
        let db = Arc::emplace(ReadOnlyDbWrapper::new());
        let mut db = Pin::into_inner(db);
        let db_mut = Arc::get_mut(&mut db).unwrap();
        moveit! {
            let status = Pin::new(db_mut).open(options);
        }
        into_result(&status)?;
        Ok(ReadOnlyDb { inner: db })
    }

    pub fn default_col(&self) -> usize {
        self.inner.default_col()
    }

    pub fn get<'a>(
        &'a self,
        col: usize,
        key: &[u8],
        slot: Slot<'a, PinnableSlice>,
    ) -> Result<Option<PinnedSlice<'a>>> {
        moveit! {
            let options = ReadOptions::new();
        }
        self.get_with_options(&options, col, key, slot)
    }

    pub fn get_with_options<'a>(
        &'a self,
        options: &ReadOptions,
        col: usize,
        key: &[u8],
        slot: Slot<'a, PinnableSlice>,
    ) -> Result<Option<PinnedSlice<'a>>> {
        let cf = self.inner.get_cf(col);
        assert!(!cf.is_null());
        let mut slice = slot.emplace(PinnableSlice::new());
        let slice_ptr = unsafe { slice.as_mut().get_unchecked_mut() };
        moveit! {
            let status = unsafe { self.inner.get(options, cf, &key.into(), slice_ptr) };
        }
        if status.IsNotFound() {
            return Ok(None);
        }
        into_result(&status)?;
        Ok(Some(PinnedSlice::new(slice)))
    }

    pub fn iter(&self, col: usize, dir: Direction) -> DbIterator<&'_ Self> {
        moveit! {
            let options = ReadOptions::new();
        }
        self.iter_with_options(&options, col, dir)
    }

    pub fn iter_with_options<'a>(
        &'a self,
        options: &ReadOptions,
        col: usize,
        dir: Direction,
    ) -> DbIterator<&'a Self> {
        let cf = self.inner.get_cf(col);
        assert!(!cf.is_null());
        unsafe { DbIterator::new(self.as_inner().iter(options, cf), dir) }
    }

    pub fn as_inner(&self) -> &ReadOnlyDbWrapper {
        &self.inner
    }
}
