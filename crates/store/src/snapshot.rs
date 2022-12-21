use autorocks::{moveit::slot, Direction, Snapshot};

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
        slot!(slice);
        self.inner
            .get(col, key, slice)
            .expect("db operation should be ok")
            .map(|p| p.as_ref().into())
    }
}

impl StoreSnapshot {
    pub fn iter_mem_pool_transactions(&self) -> impl Iterator<Item = Box<[u8]>> + '_ {
        self.inner
            .iter(COLUMN_MEM_POOL_TRANSACTION, Direction::Forward)
            .map(|(k, _)| k)
    }
}
