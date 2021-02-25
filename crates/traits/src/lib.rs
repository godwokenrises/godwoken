use gw_common::H256;
use gw_db::error::Error as DBError;
use gw_types::{
    bytes::Bytes,
    packed::{self, Script},
};

pub trait CodeStore {
    fn insert_script(&mut self, script_hash: H256, script: Script);
    fn get_script(&self, script_hash: &H256) -> Option<Script>;
    fn insert_data(&mut self, data_hash: H256, code: Bytes);
    fn get_data(&self, data_hash: &H256) -> Option<Bytes>;
}

pub trait ChainStore {
    fn get_tip_block_hash(&self) -> Result<H256, DBError>;
    fn get_block_hash_by_number(&self, number: u64) -> Result<Option<H256>, DBError>;
    fn get_block_number(&self, block_hash: &H256) -> Result<Option<u64>, DBError>;
    fn get_block_by_number(&self, number: u64) -> Result<Option<packed::L2Block>, DBError>;
    fn get_block(&self, block_hash: &H256) -> Result<Option<packed::L2Block>, DBError>;
    fn get_transaction(&self, tx_hash: &H256) -> Result<Option<packed::L2Transaction>, DBError>;
}
