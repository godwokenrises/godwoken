use autorocks::{autorocks_sys::rocksdb::PinnableSlice, moveit::moveit, SharedSnapshot};

use crate::traits::{chain_store::ChainStore, kv_store::KVStoreRead};

pub struct TransactionSnapshot {
    pub(super) inner: SharedSnapshot,
}

impl KVStoreRead for TransactionSnapshot {
    fn get(&self, col: crate::schema::Col, key: &[u8]) -> Option<Box<[u8]>> {
        moveit! {
            let mut buf = PinnableSlice::new();
        }
        self.inner
            .get(col, key, buf.as_mut())
            .unwrap()
            .map(Into::into)
    }
}

impl ChainStore for TransactionSnapshot {}
