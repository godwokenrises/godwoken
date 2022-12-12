use std::{hint::unreachable_unchecked, pin::Pin};

use autocxx::prelude::UniquePtr;
use moveit::moveit;

use crate::{into_result, Result, TransactionDb};

pub struct WriteBatch {
    pub(crate) inner: UniquePtr<autorocks_sys::rocksdb::WriteBatch>,
    pub(crate) db: TransactionDb,
}

impl WriteBatch {
    pub fn put(&mut self, col: usize, key: &[u8], value: &[u8]) -> Result<()> {
        let cf = self.db.as_inner().get_cf(col);
        assert!(!cf.is_null());
        moveit! {
            let status = unsafe { self.as_inner_mut().Put(cf, &key.into(), &value.into()) };
        }
        into_result(&status)
    }

    pub fn delete(&mut self, col: usize, key: &[u8]) -> Result<()> {
        let cf = self.db.as_inner().get_cf(col);
        assert!(!cf.is_null());
        moveit! {
            let status = unsafe { self.as_inner_mut().Delete(cf, &key.into()) };
        }
        into_result(&status)
    }

    /// Delete entries in the range of ["begin_key", "end_key").
    pub fn delete_range(&mut self, col: usize, begin_key: &[u8], end_key: &[u8]) -> Result<()> {
        let cf = self.db.as_inner().get_cf(col);
        assert!(!cf.is_null());
        moveit! {
            let status = unsafe { self.as_inner_mut().DeleteRange(cf, &begin_key.into(), &end_key.into()) };
        }
        into_result(&status)
    }

    pub fn as_inner_mut(&mut self) -> Pin<&mut autorocks_sys::rocksdb::WriteBatch> {
        match self.inner.as_mut() {
            Some(x) => x,
            None => unsafe { unreachable_unchecked() },
        }
    }

    pub fn as_inner(&self) -> &autorocks_sys::rocksdb::WriteBatch {
        &self.inner
    }
}
