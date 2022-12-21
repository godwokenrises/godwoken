use std::{hint::unreachable_unchecked, marker::PhantomData};

use autocxx::cxx::SharedPtr;
use autorocks_sys::{rocksdb::PinnableSlice, ReadOptionsWrapper};
use moveit::{moveit, Slot};

use crate::{slice::PinnedSlice, DbIterator, Direction, Result, Transaction, TransactionDb};

pub struct Snapshot {
    pub(crate) inner: *const autorocks_sys::rocksdb::Snapshot,
    pub(crate) db: TransactionDb,
}

unsafe impl Send for Snapshot {}
unsafe impl Sync for Snapshot {}

impl Snapshot {
    pub fn get<'a>(
        &'a self,
        col: usize,
        key: &[u8],
        slot: Slot<'a, PinnableSlice>,
    ) -> Result<Option<PinnedSlice<'a>>> {
        moveit! {
            let mut options = ReadOptionsWrapper::new();
        }
        unsafe {
            options.as_mut().set_snapshot(self.inner);
        }
        self.db
            .get_with_options((*options).as_ref(), col, key, slot)
    }

    pub fn iter(&self, col: usize, dir: Direction) -> DbIterator<&'_ Self> {
        moveit! {
            let mut options = ReadOptionsWrapper::new();
        }
        unsafe {
            options.as_mut().set_snapshot(self.inner);
        }
        let iter = self.db.iter_with_options((*options).as_ref(), col, dir);
        DbIterator {
            inner: iter.inner,
            just_seeked: iter.just_seeked,
            direction: iter.direction,
            phantom: PhantomData,
        }
    }
}

impl Drop for Snapshot {
    fn drop(&mut self) {
        unsafe {
            self.db.as_inner().release_snapshot(self.inner);
        }
    }
}

#[derive(Clone)]
pub struct SharedSnapshot {
    /// Safety: inner must not be null.
    pub(crate) inner: SharedPtr<autorocks_sys::rocksdb::Snapshot>,
    pub(crate) db: TransactionDb,
}

unsafe impl Send for SharedSnapshot {}
unsafe impl Sync for SharedSnapshot {}

impl SharedSnapshot {
    fn as_inner(&self) -> &autorocks_sys::rocksdb::Snapshot {
        match self.inner.as_ref() {
            Some(snap) => snap,
            None => unsafe { unreachable_unchecked() },
        }
    }

    pub fn get<'a>(
        &'a self,
        col: usize,
        key: &[u8],
        slot: Slot<'a, PinnableSlice>,
    ) -> Result<Option<PinnedSlice<'a>>> {
        moveit! {
            let mut options = ReadOptionsWrapper::new();
        }
        unsafe {
            options.as_mut().set_snapshot(self.as_inner());
        }
        self.db
            .get_with_options((*options).as_ref(), col, key, slot)
    }

    pub fn iter(&self, col: usize, dir: Direction) -> DbIterator<&'_ Self> {
        moveit! {
            let mut options = ReadOptionsWrapper::new();
        }
        unsafe {
            options.as_mut().set_snapshot(self.as_inner());
        }
        let iter = self.db.iter_with_options((*options).as_ref(), col, dir);
        DbIterator {
            inner: iter.inner,
            just_seeked: iter.just_seeked,
            direction: iter.direction,
            phantom: PhantomData,
        }
    }
}

pub struct SnapshotRef<'a> {
    pub(crate) inner: &'a autorocks_sys::rocksdb::Snapshot,
    pub(crate) tx: &'a Transaction,
}

unsafe impl Send for SnapshotRef<'_> {}
unsafe impl Sync for SnapshotRef<'_> {}

impl<'a> SnapshotRef<'a> {
    pub fn get(
        &'a self,
        col: usize,
        key: &[u8],
        slot: Slot<'a, PinnableSlice>,
    ) -> Result<Option<PinnedSlice<'a>>> {
        moveit! {
            let mut options = ReadOptionsWrapper::new();
        }
        unsafe {
            options.as_mut().set_snapshot(self.inner);
        }
        self.tx
            .get_with_options((*options).as_ref(), col, key, slot)
    }

    pub fn iter(&self, col: usize, dir: Direction) -> DbIterator<&'_ Self> {
        moveit! {
            let mut options = ReadOptionsWrapper::new();
        }
        unsafe {
            options.as_mut().set_snapshot(self.inner);
        }
        let iter = self.tx.iter_with_options((*options).as_ref(), col, dir);
        DbIterator {
            inner: iter.inner,
            just_seeked: iter.just_seeked,
            direction: iter.direction,
            phantom: PhantomData,
        }
    }
}
