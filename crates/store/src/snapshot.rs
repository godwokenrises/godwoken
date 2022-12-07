use anyhow::{bail, Result};
use autorocks::{autorocks_sys::rocksdb::PinnableSlice, moveit::moveit, Direction, Snapshot};

use crate::{
    schema::{Col, COLUMN_MEM_POOL_TRANSACTION},
    traits::{
        chain_store::ChainStore,
        kv_store::{KVStore, KVStoreRead, KVStoreWrite},
    },
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

/// We implement the write for snapshot for readonly operations
impl KVStoreWrite for StoreSnapshot {
    fn insert_raw(&self, _col: Col, _key: &[u8], _value: &[u8]) -> Result<()> {
        bail!("Can't write to snapshot")
    }

    fn delete(&self, _col: Col, _key: &[u8]) -> Result<()> {
        bail!("Can't delete key from snapshot")
    }
}

impl KVStore for StoreSnapshot {}

impl StoreSnapshot {
    pub fn iter_mem_pool_transactions(&self) -> impl Iterator<Item = Box<[u8]>> + '_ {
        self.inner
            .iter(COLUMN_MEM_POOL_TRANSACTION, Direction::Forward)
            .map(|(k, _)| k)
    }
}
