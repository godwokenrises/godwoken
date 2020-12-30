use crate::db_utils::build_transaction_key;
use gw_common::H256;
use gw_db::{
    error::Error,
    schema::{Col, COLUMN_BLOCK, COLUMN_NUMBER_HASH},
    RocksDBWriteBatch,
};

pub struct StoreWriteBatch {
    pub(crate) inner: RocksDBWriteBatch,
}

impl StoreWriteBatch {
    pub fn put(&mut self, col: Col, key: &[u8], value: &[u8]) -> Result<(), Error> {
        self.inner.put(col, key, value)
    }

    pub fn delete(&mut self, col: Col, key: &[u8]) -> Result<(), Error> {
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

    pub fn clear(&mut self) -> Result<(), Error> {
        self.inner.clear()
    }
}
