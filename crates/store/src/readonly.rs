use gw_common::H256;
use gw_db::{
    iter::DBIter,
    read_only_db::ReadOnlyDB,
    schema::{Col, COLUMN_REVERTED_BLOCK_SMT_ROOT},
    DBIterator, Direction, IteratorMode,
};
use gw_types::{
    from_box_should_be_ok, packed,
    prelude::{Entity, FromSliceShouldBeOk, Unpack},
};

use crate::traits::{chain_store::ChainStore, kv_store::KVStoreRead};

#[derive(Clone)]
pub struct StoreReadonly {
    inner: ReadOnlyDB,
}

impl StoreReadonly {
    pub fn new(inner: ReadOnlyDB) -> Self {
        StoreReadonly { inner }
    }

    pub(crate) fn get_iter(&self, col: Col, mode: IteratorMode) -> DBIter {
        self.inner
            .iter(col, mode)
            .expect("db operation should be ok")
    }

    pub fn iter_reverted_block_smt_root(
        &self,
        root: H256,
    ) -> impl Iterator<Item = (H256, Vec<H256>)> + '_ {
        self.get_iter(
            COLUMN_REVERTED_BLOCK_SMT_ROOT,
            IteratorMode::From(root.as_slice(), Direction::Reverse),
        )
        .map(|(root_slice, hashes_slice)| {
            let root = from_box_should_be_ok!(packed::Byte32Reader, root_slice);
            let hashes = from_box_should_be_ok!(packed::Byte32VecReader, hashes_slice);
            (root.unpack(), hashes.unpack())
        })
    }
}

impl ChainStore for StoreReadonly {}

impl KVStoreRead for StoreReadonly {
    fn get(&self, col: Col, key: &[u8]) -> Option<Box<[u8]>> {
        self.inner
            .get_pinned(col, key)
            .expect("db operation should be ok")
            .map(|v| Box::<[u8]>::from(v.as_ref()))
    }
}
