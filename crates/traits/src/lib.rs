use gw_common::H256;
use gw_db::error::Error as DBError;
use gw_types::{bytes::Bytes, packed::Script};

pub trait CodeStore {
    fn insert_script(&mut self, script_hash: H256, script: Script);
    fn get_script(&self, script_hash: &H256) -> Option<Script>;
    fn get_script_hash_by_short_script_hash(&self, short_script_hash: &[u8]) -> Option<H256>;
    fn insert_data(&mut self, data_hash: H256, code: Bytes);
    fn get_data(&self, data_hash: &H256) -> Option<Bytes>;
}

pub trait ChainView {
    fn get_block_hash_by_number(&self, number: u64) -> Result<Option<H256>, DBError>;
}
