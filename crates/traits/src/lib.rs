use std::cell::RefCell;

use anyhow::Result;
use gw_common::H256;
use gw_types::{bytes::Bytes, packed::Script};

pub trait CodeStore {
    fn insert_script(&mut self, script_hash: H256, script: Script);
    fn get_script(&self, script_hash: &H256) -> Option<Script>;
    fn insert_data(&mut self, data_hash: H256, code: Bytes);
    fn get_data(&self, data_hash: &H256) -> Option<Bytes>;
}

pub trait ChainView {
    fn get_block_hash_by_number(&self, number: u64) -> Result<Option<H256>>;
}

impl<T: CodeStore> CodeStore for &mut T {
    fn insert_script(&mut self, script_hash: H256, script: Script) {
        <T as CodeStore>::insert_script(self, script_hash, script)
    }
    fn get_script(&self, script_hash: &H256) -> Option<Script> {
        <T as CodeStore>::get_script(self, script_hash)
    }
    fn insert_data(&mut self, data_hash: H256, code: Bytes) {
        <T as CodeStore>::insert_data(self, data_hash, code)
    }
    fn get_data(&self, data_hash: &H256) -> Option<Bytes> {
        <T as CodeStore>::get_data(self, data_hash)
    }
}

impl<T: CodeStore> CodeStore for &RefCell<T> {
    fn insert_script(&mut self, script_hash: H256, script: Script) {
        <T as CodeStore>::insert_script(&mut self.borrow_mut(), script_hash, script)
    }
    fn get_script(&self, script_hash: &H256) -> Option<Script> {
        <T as CodeStore>::get_script(&self.borrow(), script_hash)
    }
    fn insert_data(&mut self, data_hash: H256, code: Bytes) {
        <T as CodeStore>::insert_data(&mut self.borrow_mut(), data_hash, code)
    }
    fn get_data(&self, data_hash: &H256) -> Option<Bytes> {
        <T as CodeStore>::get_data(&self.borrow(), data_hash)
    }
}

impl<T: ChainView> ChainView for &T {
    fn get_block_hash_by_number(&self, number: u64) -> Result<Option<H256>> {
        <T as ChainView>::get_block_hash_by_number(self, number)
    }
}
