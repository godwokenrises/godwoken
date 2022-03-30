//! ChainView implement ChainStore

use gw_common::H256;
use gw_db::error::Error;
use gw_traits::ChainView as ChainViewTrait;

use crate::traits::chain_store::ChainStore;

/// Max block hashes we can read, not included tip
const MAX_BLOCK_HASHES_DEPTH: u64 = 256;

pub struct ChainView<'db, DB> {
    db: &'db DB,
    tip_block_hash: H256,
}

impl<'db, DB: ChainStore> ChainView<'db, DB> {
    pub fn new(db: &'db DB, tip_block_hash: H256) -> Self {
        Self { db, tip_block_hash }
    }
}

impl<'db, DB: ChainStore> ChainViewTrait for ChainView<'db, DB> {
    fn get_block_hash_by_number(&self, number: u64) -> Result<Option<H256>, Error> {
        // if we can read block number from db index, we are in the main chain
        if let Some(tip_number) = self.db.get_block_number(&self.tip_block_hash)? {
            if !is_number_in_a_valid_range(tip_number, number) {
                return Ok(None);
            }
            if tip_number == number {
                return Ok(Some(self.tip_block_hash));
            }
            // so we can direct return a block hash from db index
            return self.db.get_block_hash_by_number(number);
        }

        // we are on a forked chain
        // since we always execute transactions based on main chain
        // it is a bug in the current version
        Err("shouldn't execute transaction on forked chain"
            .to_string()
            .into())
    }
}

fn is_number_in_a_valid_range(tip_number: u64, number: u64) -> bool {
    number <= tip_number && number >= tip_number.saturating_sub(MAX_BLOCK_HASHES_DEPTH)
}
