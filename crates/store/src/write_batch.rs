use anyhow::Result;
use gw_db::{schema::Col, RocksDBWriteBatch};

pub struct StoreWriteBatch {
    pub(crate) inner: RocksDBWriteBatch,
}

impl StoreWriteBatch {
    pub fn put(&mut self, col: Col, key: &[u8], value: &[u8]) -> Result<()> {
        self.inner.put(col, key, value)
    }

    pub fn delete(&mut self, col: Col, key: &[u8]) -> Result<()> {
        self.inner.delete(col, key)
    }

    pub fn size_in_bytes(&self) -> usize {
        self.inner.size_in_bytes()
    }

    pub fn len(&self) -> usize {
        self.inner.len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    pub fn clear(&mut self) -> Result<()> {
        self.inner.clear()
    }
}
