use gw_common::{
    error::Error,
    smt::{default_store::DefaultStore, H256, SMT},
    state::State,
};
use gw_traits::CodeStore;
use gw_types::{bytes::Bytes, packed::Script};
use std::collections::HashMap;

#[derive(Default)]
pub struct DummyState {
    tree: SMT<DefaultStore<H256>>,
    account_count: u32,
    scripts: HashMap<H256, Script>,
    codes: HashMap<H256, Bytes>,
}

impl State for DummyState {
    fn get_raw(&self, key: &H256) -> Result<H256, Error> {
        let v = self.tree.get(key)?;
        Ok(v)
    }
    fn update_raw(&mut self, key: H256, value: H256) -> Result<(), Error> {
        self.tree.update(key, value)?;
        Ok(())
    }
    fn calculate_root(&self) -> Result<H256, Error> {
        let root = *self.tree.root();
        Ok(root)
    }
    fn get_account_count(&self) -> Result<u32, Error> {
        Ok(self.account_count)
    }
    fn set_account_count(&mut self, count: u32) -> Result<(), Error> {
        self.account_count = count;
        Ok(())
    }
}

impl CodeStore for DummyState {
    fn insert_script(&mut self, script_hash: H256, script: Script) {
        self.scripts.insert(script_hash, script);
    }
    fn get_script(&self, script_hash: &H256) -> Option<Script> {
        self.scripts.get(&script_hash).cloned()
    }
    fn get_script_hash_by_prefix(&self, script_hash_prefix: &[u8]) -> Option<H256> {
        self.scripts.iter().find_map(|(script_hash, _script)| {
            if script_hash.as_slice() == script_hash_prefix {
                Some(*script_hash)
            } else {
                None
            }
        })
    }
    fn insert_data(&mut self, script_hash: H256, code: Bytes) {
        self.codes.insert(script_hash, code);
    }
    fn get_data(&self, script_hash: &H256) -> Option<Bytes> {
        self.codes.get(script_hash).cloned()
    }
}
