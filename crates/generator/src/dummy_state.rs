use crate::traits::CodeStore;
use gw_common::{
    smt::{default_store::DefaultStore, H256, SMT},
    state::{Error, State},
};
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
        let v = self.tree.get(&(*key).into())?;
        Ok(v.into())
    }
    fn update_raw(&mut self, key: H256, value: H256) -> Result<(), Error> {
        self.tree.update(key.into(), value.into())?;
        Ok(())
    }
    fn calculate_root(&self) -> Result<H256, Error> {
        let root = (*self.tree.root()).into();
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
        self.scripts.insert(script_hash.into(), script);
    }
    fn insert_code(&mut self, code_hash: H256, code: Bytes) {
        self.codes.insert(code_hash.into(), code);
    }
    fn get_script(&self, script_hash: &H256) -> Option<Script> {
        self.scripts.get(&script_hash).cloned()
    }
    fn get_code(&self, code_hash: &H256) -> Option<Bytes> {
        self.codes.get(&code_hash).cloned()
    }
}
