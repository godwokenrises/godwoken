use autorocks::{moveit::slot, SharedSnapshot};

use crate::traits::{chain_store::ChainStore, kv_store::KVStoreRead};

pub struct TransactionSnapshot {
    pub(super) inner: SharedSnapshot,
}

impl KVStoreRead for TransactionSnapshot {
    fn get(&self, col: crate::schema::Col, key: &[u8]) -> Option<Box<[u8]>> {
        slot!(slice);
        self.inner
            .get(col, key, slice)
            .unwrap()
            .map(|p| p.as_ref().into())
    }
}

impl ChainStore for TransactionSnapshot {}
