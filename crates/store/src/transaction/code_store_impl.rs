use gw_common::H256;
use gw_traits::CodeStore;
use gw_types::{bytes::Bytes, from_box_should_be_ok, packed, prelude::*};

use crate::{
    schema::{COLUMN_DATA, COLUMN_SCRIPT},
    traits::kv_store::{KVStoreRead, KVStoreWrite},
};

use super::StoreTransaction;

impl CodeStore for &StoreTransaction {
    fn insert_script(&mut self, script_hash: H256, script: packed::Script) {
        self.insert_raw(COLUMN_SCRIPT, script_hash.as_slice(), script.as_slice())
            .expect("insert script");
    }

    fn get_script(&self, script_hash: &H256) -> Option<packed::Script> {
        self.get(COLUMN_SCRIPT, script_hash.as_slice())
            .map(|slice| from_box_should_be_ok!(packed::ScriptReader, slice))
    }

    fn insert_data(&mut self, data_hash: H256, code: Bytes) {
        self.insert_raw(COLUMN_DATA, data_hash.as_slice(), &code)
            .expect("insert data");
    }

    fn get_data(&self, data_hash: &H256) -> Option<Bytes> {
        self.get(COLUMN_DATA, data_hash.as_slice())
            .map(|slice| Bytes::from(slice.to_vec()))
    }
}
