use anyhow::Result;
use gw_db::schema::Col;

pub trait KVStoreRead {
    fn get(&self, col: Col, key: &[u8]) -> Option<Box<[u8]>>;
}

pub trait KVStoreWrite {
    fn insert_raw(&self, col: Col, key: &[u8], value: &[u8]) -> Result<()>;
    fn delete(&self, col: Col, key: &[u8]) -> Result<()>;
}

pub trait KVStore: KVStoreRead + KVStoreWrite {}
