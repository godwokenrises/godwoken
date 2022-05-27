use std::convert::TryInto;
use std::sync::Arc;
use std::{collections::HashMap, sync::RwLock};

use anyhow::Result;
use gw_common::error::Error as StateError;
use gw_common::registry_address::RegistryAddress;
use gw_common::smt::SMT;
use gw_common::state::State;
use gw_common::H256;
use gw_db::schema::{
    COLUMNS, COLUMN_ACCOUNT_SMT_BRANCH, COLUMN_ACCOUNT_SMT_LEAF, COLUMN_DATA, COLUMN_META,
    COLUMN_SCRIPT,
};
use gw_db::{error::Error, schema::Col};
use gw_generator::traits::StateExt;
use gw_store::mem_pool_state::{META_MEM_SMT_COUNT_KEY, META_MEM_SMT_ROOT_KEY};
use gw_store::smt::smt_store::SMTStore;
use gw_store::traits::kv_store::KVStoreWrite;
use gw_store::{
    mem_pool_state::MemStore,
    traits::{
        chain_store::ChainStore,
        kv_store::{KVStore, KVStoreRead},
    },
};
use gw_traits::CodeStore;
use gw_types::bytes::Bytes;
use gw_types::from_box_should_be_ok;
use gw_types::packed::{AccountMerkleState, Script, ScriptReader, Uint32Reader};
use gw_types::prelude::{Builder, Entity, FromSliceShouldBeOk, Pack, Unpack};

enum Value<T> {
    Exist(T),
    Deleted,
}

type MemColumn = HashMap<Vec<u8>, Value<Vec<u8>>>;

pub struct MemExecuteTxStore {
    inner: Arc<MemStore>,
    mem: Vec<RwLock<MemColumn>>,
}

impl MemExecuteTxStore {
    pub fn new(inner: Arc<MemStore>) -> Self {
        let mut mem = Vec::with_capacity(COLUMNS as usize);
        mem.resize_with(COLUMNS as usize, || RwLock::new(HashMap::default()));

        Self { inner, mem }
    }

    pub fn state(&self) -> Result<MemExecuteTxStateTree<'_>> {
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
        Ok(MemExecuteTxStateTree::new(tree, account_count))
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
            Some(slice) => Ok(Some(Uint32Reader::from_slice_should_be_ok(&slice).unpack())),
            None => Ok(None),
        }
    }
}

impl KVStoreRead for MemExecuteTxStore {
    fn get(&self, col: Col, key: &[u8]) -> Option<Box<[u8]>> {
        match self
            .mem
            .get(col as usize)
            .expect("can't found column")
            .read()
            .expect("get read lock failed")
            .get(key)
        {
            Some(Value::Exist(v)) => Some(v.clone().into_boxed_slice()),
            Some(Value::Deleted) => None,
            None => self.inner.get(col, key),
        }
    }
}

impl KVStoreWrite for MemExecuteTxStore {
    fn insert_raw(&self, col: Col, key: &[u8], value: &[u8]) -> Result<(), Error> {
        self.mem
            .get(col as usize)
            .expect("can't found column")
            .write()
            .expect("get write lock failed")
            .insert(key.to_vec(), Value::Exist(value.to_vec()));
        Ok(())
    }

    fn delete(&self, col: Col, key: &[u8]) -> Result<(), Error> {
        self.mem
            .get(col as usize)
            .expect("can't found column")
            .write()
            .expect("get write lock failed")
            .insert(key.to_vec(), Value::Deleted);
        Ok(())
    }
}

impl KVStore for MemExecuteTxStore {}

impl ChainStore for MemExecuteTxStore {}

pub struct MemExecuteTxStateTree<'a> {
    tree: SMT<SMTStore<'a, MemExecuteTxStore>>,
    account_count: u32,
}

impl<'a> MemExecuteTxStateTree<'a> {
    pub fn new(tree: SMT<SMTStore<'a, MemExecuteTxStore>>, account_count: u32) -> Self {
        MemExecuteTxStateTree {
            tree,
            account_count,
        }
    }

    pub fn get_merkle_state(&self) -> AccountMerkleState {
        AccountMerkleState::new_builder()
            .merkle_root(self.tree.root().pack())
            .count(self.account_count.pack())
            .build()
    }

    pub fn smt(&self) -> &SMT<SMTStore<'a, MemExecuteTxStore>> {
        &self.tree
    }

    pub fn mock_account(
        &mut self,
        registry_address: RegistryAddress,
        account_script: Script,
    ) -> Result<u32> {
        let account_script_hash = account_script.hash().into();
        let account_id = self.create_account_from_script(account_script)?;
        self.mapping_registry_address_to_script_hash(registry_address, account_script_hash)?;
        Ok(account_id)
    }

    fn db(&self) -> &MemExecuteTxStore {
        self.tree.store().inner_store()
    }
}

impl<'a> State for MemExecuteTxStateTree<'a> {
    fn get_raw(&self, key: &H256) -> Result<H256, StateError> {
        let v = self.tree.get(key)?;
        Ok(v)
    }

    fn update_raw(&mut self, key: H256, value: H256) -> Result<(), StateError> {
        self.tree.update(key, value)?;
        Ok(())
    }

    fn get_account_count(&self) -> Result<u32, StateError> {
        Ok(self.account_count)
    }

    fn set_account_count(&mut self, count: u32) -> Result<(), StateError> {
        self.account_count = count;
        Ok(())
    }

    fn calculate_root(&self) -> Result<H256, StateError> {
        let root = self.tree.root();
        Ok(*root)
    }
}

impl<'a> CodeStore for MemExecuteTxStateTree<'a> {
    fn insert_script(&mut self, script_hash: H256, script: Script) {
        self.db()
            .insert_raw(COLUMN_SCRIPT, script_hash.as_slice(), script.as_slice())
            .expect("insert script");
    }

    fn get_script(&self, script_hash: &H256) -> Option<Script> {
        self.db()
            .get(COLUMN_SCRIPT, script_hash.as_slice())
            .or_else(|| self.db().get(COLUMN_SCRIPT, script_hash.as_slice()))
            .map(|slice| from_box_should_be_ok!(ScriptReader, slice))
    }

    fn insert_data(&mut self, data_hash: H256, code: Bytes) {
        self.db()
            .insert_raw(COLUMN_DATA, data_hash.as_slice(), &code)
            .expect("insert data");
    }

    fn get_data(&self, data_hash: &H256) -> Option<Bytes> {
        self.db()
            .get(COLUMN_DATA, data_hash.as_slice())
            .or_else(|| self.db().get(COLUMN_DATA, data_hash.as_slice()))
            .map(|slice| Bytes::from(slice.to_vec()))
    }
}
