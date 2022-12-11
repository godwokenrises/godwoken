use autorocks::{autorocks_sys::rocksdb::PinnableSlice, moveit::moveit, Direction, Snapshot};

use crate::{
    schema::{Col, COLUMN_MEM_POOL_TRANSACTION},
    traits::{chain_store::ChainStore, kv_store::KVStoreRead},
};

pub struct StoreSnapshot {
    inner: Snapshot,
}

impl StoreSnapshot {
    pub(crate) fn new(inner: Snapshot) -> Self {
        Self { inner }
    }
}

impl ChainStore for StoreSnapshot {}

impl KVStoreRead for StoreSnapshot {
    fn get(&self, col: Col, key: &[u8]) -> Option<Box<[u8]>> {
        moveit! {
            let mut buf = PinnableSlice::new();
        }
        self.inner
            .get(col, key, buf.as_mut())
            .expect("db operation should be ok")
            .map(Into::into)
    }
}

impl StoreSnapshot {
    pub fn iter_mem_pool_transactions(&self) -> impl Iterator<Item = Box<[u8]>> + '_ {
        self.inner
            .iter(COLUMN_MEM_POOL_TRANSACTION, Direction::Forward)
            .map(|(k, _)| k)
    }
}
