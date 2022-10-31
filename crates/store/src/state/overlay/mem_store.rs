use std::{
    borrow::Borrow,
    hash::{Hash, Hasher},
    sync::{Arc, RwLock},
};

use anyhow::Result;
use gw_common::H256;
use gw_db::{
    error::Error,
    schema::{Col, COLUMN_DATA, COLUMN_SCRIPT},
};
use gw_traits::CodeStore;
use gw_types::{
    bytes::Bytes,
    from_box_should_be_ok, packed,
    prelude::{Entity, FromSliceShouldBeOk},
};
use im::HashMap;

use crate::{
    snapshot::StoreSnapshot,
    traits::{
        chain_store::ChainStore,
        kv_store::{KVStore, KVStoreRead, KVStoreWrite},
    },
};

#[derive(Clone, PartialEq, Eq, Debug)]
enum Value<T> {
    Exist(T),
    Deleted,
}

type ColumnsKeyValueMap = HashMap<(u8, Vec<u8>), Value<Vec<u8>>>;

pub struct MemStore {
    inner: Arc<StoreSnapshot>,
    // (column, key) -> value.
    mem: RwLock<ColumnsKeyValueMap>,
}

impl MemStore {
    pub fn new(inner: impl Into<Arc<StoreSnapshot>>) -> Self {
        Self {
            inner: inner.into(),
            mem: RwLock::new(HashMap::new()),
        }
    }
}

impl ChainStore for MemStore {}

impl CodeStore for MemStore {
    fn insert_script(&mut self, script_hash: H256, script: packed::Script) {
        self.insert_raw(COLUMN_SCRIPT, script_hash.as_slice(), script.as_slice())
            .expect("insert script")
    }

    fn get_script(&self, script_hash: &H256) -> Option<packed::Script> {
        self.get(COLUMN_SCRIPT, script_hash.as_slice())
            .map(|slice| from_box_should_be_ok!(packed::ScriptReader, slice))
    }

    fn insert_data(&mut self, data_hash: H256, code: gw_types::bytes::Bytes) {
        self.insert_raw(COLUMN_DATA, data_hash.as_slice(), &code)
            .expect("insert data");
    }

    fn get_data(&self, data_hash: &H256) -> Option<gw_types::bytes::Bytes> {
        self.get(COLUMN_DATA, data_hash.as_slice())
            .map(|slice| Bytes::from(slice.to_vec()))
    }
}

impl KVStoreRead for MemStore {
    fn get(&self, col: Col, key: &[u8]) -> Option<Box<[u8]>> {
        match self
            .mem
            .read()
            .expect("get read lock failed")
            .get(&(col, key) as &dyn Key)
        {
            Some(Value::Exist(v)) => Some(v.clone().into_boxed_slice()),
            Some(Value::Deleted) => None,
            None => self.inner.get(col, key),
        }
    }
}

impl KVStoreWrite for MemStore {
    fn insert_raw(&self, col: Col, key: &[u8], value: &[u8]) -> Result<(), Error> {
        self.mem
            .write()
            .expect("get write lock failed")
            .insert((col, key.into()), Value::Exist(value.to_vec()));
        Ok(())
    }

    fn delete(&self, col: Col, key: &[u8]) -> Result<(), Error> {
        self.mem
            .write()
            .expect("get write lock failed")
            .insert((col, key.into()), Value::Deleted);
        Ok(())
    }
}

impl KVStore for MemStore {}

impl Clone for MemStore {
    /// Make a clone of the store. This is cheap.
    ///
    /// Modifications on the clone will NOT be seen on this store.
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            mem: RwLock::new(self.mem.read().unwrap().clone()),
        }
    }
}

// So that we can query a ColumnsKeyValueMap with (u8, &[u8]), without temporary
// allocation.
//
// https://stackoverflow.com/questions/36480845/how-to-avoid-temporary-allocations-when-using-a-complex-key-for-a-hashmap/50478038#50478038
trait Key {
    fn to_key(&self) -> (u8, &[u8]);
}

impl Hash for dyn Key + '_ {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.to_key().hash(state)
    }
}

impl PartialEq for dyn Key + '_ {
    fn eq(&self, other: &Self) -> bool {
        self.to_key() == other.to_key()
    }
}

impl Eq for dyn Key + '_ {}

impl Key for (u8, Vec<u8>) {
    fn to_key(&self) -> (u8, &[u8]) {
        (self.0, &self.1[..])
    }
}

impl<'a> Key for (u8, &'a [u8]) {
    fn to_key(&self) -> (u8, &[u8]) {
        *self
    }
}

impl<'a> Borrow<dyn Key + 'a> for (u8, Vec<u8>) {
    fn borrow(&self) -> &(dyn Key + 'a) {
        self
    }
}
