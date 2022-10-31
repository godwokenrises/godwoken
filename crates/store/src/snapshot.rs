use gw_db::{
    schema::{Col, COLUMN_MEM_POOL_TRANSACTION},
    DBIterator, RocksDBSnapshot,
};

use crate::traits::{
    chain_store::ChainStore,
    kv_store::{KVStore, KVStoreRead, KVStoreWrite},
};

pub struct StoreSnapshot {
    inner: RocksDBSnapshot,
}

impl StoreSnapshot {
    pub(crate) fn new(inner: RocksDBSnapshot) -> Self {
        Self { inner }
    }
}

impl ChainStore for StoreSnapshot {}

impl KVStoreRead for StoreSnapshot {
    fn get(&self, col: Col, key: &[u8]) -> Option<Box<[u8]>> {
        self.inner
            .get_pinned(col, key)
            .expect("db operation should be ok")
            .map(|v| Box::<[u8]>::from(v.as_ref()))
    }
}

/// We implement the write for snapshot for readonly operations
impl KVStoreWrite for StoreSnapshot {
    fn insert_raw(&self, _col: Col, _key: &[u8], _value: &[u8]) -> Result<(), gw_db::error::Error> {
        Err("Can't write to snapshot".to_string().into())
    }

    fn delete(&self, _col: Col, _key: &[u8]) -> Result<(), gw_db::error::Error> {
        Err("Can't delete key from snapshot".to_string().into())
    }
}

impl KVStore for StoreSnapshot {}

impl StoreSnapshot {
    pub fn iter_mem_pool_transactions(&self) -> impl Iterator<Item = Box<[u8]>> + '_ {
        self.inner
            .iter(COLUMN_MEM_POOL_TRANSACTION, gw_db::IteratorMode::Start)
            .expect("db read should not fail")
            .map(|(k, _)| k)
    }
}
