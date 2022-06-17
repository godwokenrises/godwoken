use std::{
    borrow::Borrow,
    convert::TryInto,
    hash::{Hash, Hasher},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, RwLock,
    },
};

use anyhow::Result;
use arc_swap::ArcSwap;
use gw_common::{smt::SMT, H256};
use gw_db::{
    error::Error,
    schema::{Col, COLUMN_ACCOUNT_SMT_BRANCH, COLUMN_ACCOUNT_SMT_LEAF, COLUMN_META},
};
use gw_types::{
    from_box_should_be_ok, packed,
    prelude::{Entity, FromSliceShouldBeOk, Pack, Unpack},
};
use im::HashMap;

use crate::{
    smt::smt_store::SMTStore,
    snapshot::StoreSnapshot,
    state::mem_state_db::MemStateTree,
    traits::{
        chain_store::ChainStore,
        kv_store::{KVStore, KVStoreRead, KVStoreWrite},
    },
};

pub const META_MEM_BLOCK_INFO: &[u8] = b"MEM_BLOCK_INFO";
/// account SMT root
pub const META_MEM_SMT_ROOT_KEY: &[u8] = b"MEM_ACCOUNT_SMT_ROOT_KEY";
/// account SMT count
pub const META_MEM_SMT_COUNT_KEY: &[u8] = b"MEM_ACCOUNT_SMT_COUNT_KEY";

pub struct MemPoolState {
    store: ArcSwap<MemStore>,
    completed_initial_syncing: AtomicBool,
}

impl MemPoolState {
    pub fn new(store: Arc<MemStore>, completed_initial_syncing: bool) -> Self {
        Self {
            store: ArcSwap::new(store),
            completed_initial_syncing: AtomicBool::new(completed_initial_syncing),
        }
    }

    /// Create a snapshot of the current state.
    ///
    /// Each `MemStore` loaded will be independent — updates on one `MemStore`
    /// won't be seen by other `MemStore`s.
    ///
    /// Note that updates will not be stored in `MemPoolState` unless you call
    /// [`store`].
    pub fn load(&self) -> MemStore {
        MemStore::clone(&self.store.load())
    }

    /// Replaces the snapshot inside this instance.
    pub fn store(&self, mem_store: Arc<MemStore>) {
        self.store.store(mem_store);
    }

    pub fn completed_initial_syncing(&self) -> bool {
        self.completed_initial_syncing.load(Ordering::SeqCst)
    }

    pub fn set_completed_initial_syncing(&self) {
        self.completed_initial_syncing.store(true, Ordering::SeqCst);
    }
}

#[derive(Clone)]
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

    pub fn state(&self) -> Result<MemStateTree<'_>> {
        let merkle_root = {
            let block = self.get_tip_block()?;
            block.raw().post_account()
        };
        let root = self
            .get_mem_block_account_smt_root()?
            .unwrap_or_else(|| merkle_root.merkle_root().unpack());
        let account_count = self
            .get_mem_block_account_count()?
            .unwrap_or_else(|| merkle_root.count().unpack());
        let mem_smt_store = SMTStore::new(COLUMN_ACCOUNT_SMT_LEAF, COLUMN_ACCOUNT_SMT_BRANCH, self);
        let tree = SMT::new(root, mem_smt_store);
        Ok(MemStateTree::new(tree, account_count))
    }

    pub fn get_mem_block_account_smt_root(&self) -> Result<Option<H256>, Error> {
        match self.get(COLUMN_META, META_MEM_SMT_ROOT_KEY) {
            Some(slice) => {
                let root: [u8; 32] = slice.as_ref().try_into().unwrap();
                Ok(Some(root.into()))
            }
            None => Ok(None),
        }
    }

    pub fn get_mem_block_account_count(&self) -> Result<Option<u32>, Error> {
        match self.get(COLUMN_META, META_MEM_SMT_COUNT_KEY) {
            Some(slice) => Ok(Some(
                packed::Uint32Reader::from_slice_should_be_ok(&slice).unpack(),
            )),
            None => Ok(None),
        }
    }

    pub fn set_mem_block_account_smt_root(&self, root: H256) -> Result<(), Error> {
        self.insert_raw(COLUMN_META, META_MEM_SMT_ROOT_KEY, root.as_slice())?;
        Ok(())
    }

    pub fn set_mem_block_account_count(&self, count: u32) -> Result<(), Error> {
        let count: packed::Uint32 = count.pack();
        self.insert_raw(COLUMN_META, META_MEM_SMT_COUNT_KEY, count.as_slice())
            .expect("insert");
        Ok(())
    }

    pub fn update_mem_pool_block_info(&self, block_info: &packed::BlockInfo) -> Result<(), Error> {
        self.insert_raw(COLUMN_META, META_MEM_BLOCK_INFO, block_info.as_slice())
            .expect("insert");
        Ok(())
    }

    pub fn get_mem_pool_block_info(&self) -> Result<Option<packed::BlockInfo>, Error> {
        match self.get(COLUMN_META, META_MEM_BLOCK_INFO) {
            Some(slice) => Ok(Some(from_box_should_be_ok!(packed::BlockInfoReader, slice))),
            None => Ok(None),
        }
    }
}

impl ChainStore for MemStore {}

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
