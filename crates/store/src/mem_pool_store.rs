use std::{collections::HashMap, sync::Mutex};

use gw_types::bytes::Bytes;

pub const MEM_POOL_COLUMNS: usize = 6;
pub const MEM_POOL_COL_SMT_BRANCH: usize = 0;
pub const MEM_POOL_COL_SMT_LEAF: usize = 1;
pub const MEM_POOL_COL_META: usize = 2;
pub const MEM_POOL_COL_SCRIPT: usize = 3;
pub const MEM_POOL_COL_DATA: usize = 4;
pub const MEM_POOL_COL_SCRIPT_PREFIX: usize = 5;

pub const META_MEM_POOL_BLOCK_INFO: &[u8] = b"MEM_POOL_BLOCK_INFO";
/// account SMT root
pub const META_MEM_POOL_SMT_ROOT_KEY: &[u8] = b"MEM_BLOCK_ACCOUNT_SMT_ROOT_KEY";
/// account SMT count
pub const META_MEM_POOL_SMT_COUNT_KEY: &[u8] = b"MEM_BLOCK_ACCOUNT_SMT_COUNT_KEY";

#[derive(Debug, Clone)]
pub enum Value<T> {
    Exist(T),
    Deleted,
}

impl<T> Value<T> {
    pub fn to_opt(self) -> Option<T> {
        match self {
            Self::Exist(v) => Some(v),
            Self::Deleted => None,
        }
    }
}

pub struct MemPoolStore {
    inner: Vec<Mutex<HashMap<Bytes, Value<Bytes>>>>,
}

impl MemPoolStore {
    pub fn new(columns: usize) -> Self {
        let mut mem_pool_store = Vec::default();
        mem_pool_store.resize_with(columns, || Mutex::new(Default::default()));
        Self {
            inner: mem_pool_store,
        }
    }

    pub fn get(&self, col: usize, key: &[u8]) -> Option<Value<Bytes>> {
        let col = self
            .inner
            .get(col)
            .expect("col")
            .lock()
            .expect("mem pool store");
        col.get(key).cloned()
    }

    pub fn insert(&self, col: usize, key: Bytes, value: Value<Bytes>) {
        let mut col = self
            .inner
            .get(col)
            .expect("col")
            .lock()
            .expect("mem pool store");
        col.insert(key, value);
    }
}
