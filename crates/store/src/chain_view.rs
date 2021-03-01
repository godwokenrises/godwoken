//! ChainView implement ChainStore

use gw_common::H256;
use gw_db::error::Error;
use gw_traits::ChainStore;

use crate::transaction::StoreTransaction;

/// TODO implement chain view
pub struct ChainView {
    db: StoreTransaction,
    _tip_block_hash: H256,
}

impl ChainView {
    pub fn new(db: StoreTransaction, _tip_block_hash: H256) -> Self {
        Self {
            db,
            _tip_block_hash,
        }
    }
}

impl ChainStore for ChainView {
    fn get_block_hash_by_number(&self, number: u64) -> Result<Option<H256>, Error> {
        self.db.get_block_hash_by_number(number)
    }
}
