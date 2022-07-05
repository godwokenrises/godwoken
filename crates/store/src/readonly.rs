use gw_db::{read_only_db::ReadOnlyDB, schema::Col};

use crate::traits::{chain_store::ChainStore, kv_store::KVStoreRead};

pub struct StoreReadonly {
    inner: ReadOnlyDB,
}

impl StoreReadonly {
    pub fn new(inner: ReadOnlyDB) -> Self {
        StoreReadonly { inner }
    }
}

impl ChainStore for StoreReadonly {}

impl KVStoreRead for StoreReadonly {
    fn get(&self, col: Col, key: &[u8]) -> Option<Box<[u8]>> {
        self.inner
            .get_pinned(col, key)
            .expect("db operation should be ok")
            .map(|v| Box::<[u8]>::from(v.as_ref()))
    }
}
