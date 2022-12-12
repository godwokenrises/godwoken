use std::{mem::MaybeUninit, pin::Pin};

use autorocks_sys::{
    rocksdb::{PinnableSlice, ReadOptions},
    SharedSnapshotWrapper, TransactionWrapper,
};
use moveit::{moveit, New};

use crate::{
    into_result, slice::as_rust_slice, DbIterator, Direction, Result, SharedSnapshot, SnapshotRef,
    TransactionDb,
};

pub struct Transaction {
    pub(crate) inner: TransactionWrapper,
    pub(crate) db: TransactionDb,
}

impl Transaction {
    pub fn put(&mut self, col: usize, key: &[u8], value: &[u8]) -> Result<()> {
        let cf = self.db.as_inner().get_cf(col);
        assert!(!cf.is_null());
        moveit! {
            let status = unsafe { self.as_inner_mut().put(cf, &key.into(), &value.into()) };
        }
        into_result(&status)
    }

    pub fn delete(&mut self, col: usize, key: &[u8]) -> Result<()> {
        let cf = self.db.as_inner().get_cf(col);
        assert!(!cf.is_null());
        moveit! {
            let status = unsafe { self.as_inner_mut().del(cf, &key.into()) };
        }
        into_result(&status)
    }

    pub fn get<'b>(
        &self,
        col: usize,
        key: &[u8],
        buf: Pin<&'b mut PinnableSlice>,
    ) -> Result<Option<&'b [u8]>> {
        moveit! {
            let options = ReadOptions::new();
        }
        self.get_with_options(&options, col, key, buf)
    }

    pub fn get_with_options<'b>(
        &self,
        options: &ReadOptions,
        col: usize,
        key: &[u8],
        buf: Pin<&'b mut PinnableSlice>,
    ) -> Result<Option<&'b [u8]>> {
        let slice = unsafe { buf.get_unchecked_mut() };
        let cf = self.db.as_inner().get_cf(col);
        assert!(!cf.is_null());
        moveit! {
            let status = unsafe { self.as_inner().get(options, cf, &key.into(), slice) };
        }
        if status.IsNotFound() {
            return Ok(None);
        }
        into_result(&status)?;
        Ok(Some(as_rust_slice(slice)))
    }

    /// # Panics
    ///
    /// If there are no snapshot set for this transaction.
    pub fn snapshot(&self) -> SnapshotRef<'_> {
        let snap = self.as_inner().snapshot();
        SnapshotRef {
            inner: unsafe { snap.as_ref() }.unwrap(),
            tx: self,
        }
    }

    /// Similar to `snapshot`, but the returned snapshot can outlive the
    /// transaction.
    ///
    /// # Panics
    ///
    /// If there are no snapshot set for this transaction.
    pub fn timestamped_snapshot(&self) -> SharedSnapshot {
        let mut snap: MaybeUninit<SharedSnapshotWrapper> = MaybeUninit::uninit();
        unsafe {
            self.as_inner()
                .timestamped_snapshot()
                .new(Pin::new(&mut snap));
        }
        let snap = unsafe { snap.assume_init() };
        assert!(!snap.get().is_null());
        SharedSnapshot {
            inner: snap,
            db: self.db.clone(),
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
        let cf = self.db.as_inner().get_cf(col);
        assert!(!cf.is_null());
        unsafe { DbIterator::new(self.as_inner().iter(options, cf), dir) }
    }

    pub fn rollback(&mut self) -> Result<()> {
        moveit! {
            let status = self.as_inner_mut().rollback();
        }
        into_result(&status)
    }

    pub fn commit(&mut self) -> Result<()> {
        moveit! {
            let status = self.as_inner_mut().commit();
        }
        into_result(&status)
    }

    fn as_inner(&self) -> &TransactionWrapper {
        &self.inner
    }

    fn as_inner_mut(&mut self) -> Pin<&mut TransactionWrapper> {
        Pin::new(&mut self.inner)
    }
}
