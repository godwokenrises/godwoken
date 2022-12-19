use std::path::Path;

use anyhow::Result;
use autorocks::{autorocks_sys::rocksdb::PinnableSlice, moveit::moveit, DbOptions, ReadOnlyDb};
use gw_types::{
    from_box_should_be_ok,
    h256::H256,
    packed,
    prelude::{Entity, FromSliceShouldBeOk, Unpack},
};

use crate::{
    schema::{Col, COLUMN_REVERTED_BLOCK_SMT_ROOT},
    traits::{chain_store::ChainStore, kv_store::KVStoreRead},
};

#[derive(Clone)]
pub struct StoreReadonly {
    inner: ReadOnlyDb,
}

impl StoreReadonly {
    pub fn new(inner: ReadOnlyDb) -> Self {
        StoreReadonly { inner }
    }

    pub fn open(path: &Path, columns: usize) -> Result<Self> {
        let db = DbOptions::new(path, columns).open_read_only()?;
        Ok(Self::new(db))
    }

    pub fn iter_reverted_block_smt_root(
        &self,
        root: H256,
    ) -> impl Iterator<Item = (H256, Vec<H256>)> + '_ {
        RervertedBlockHashesIter {
            snap: self,
            next_root: root,
        }
    }
}

impl ChainStore for StoreReadonly {}

impl KVStoreRead for StoreReadonly {
    fn get(&self, col: Col, key: &[u8]) -> Option<Box<[u8]>> {
        moveit! {
            let mut buf = PinnableSlice::new();
        }
        self.inner
            .get(col, key, buf.as_mut())
            .expect("db operation should be ok")
            .map(Into::into)
    }
}

pub struct RervertedBlockHashesIter<'a> {
    snap: &'a StoreReadonly,
    next_root: H256,
}

impl<'a> Iterator for RervertedBlockHashesIter<'a> {
    type Item = (H256, Vec<H256>);

    fn next(&mut self) -> Option<Self::Item> {
        let snap = &self.snap;
        let root = self.next_root;

        snap.get(COLUMN_REVERTED_BLOCK_SMT_ROOT, root.as_slice())
            .map(|slice| {
                let mut block_hashes: Vec<_> =
                    from_box_should_be_ok!(packed::Byte32VecReader, slice).unpack();

                // First hash is root
                let last_hash_idx = block_hashes.len().saturating_sub(1);
                block_hashes.swap(0, last_hash_idx);
                self.next_root = block_hashes.pop().expect("iter prev reverted block root");

                (root, block_hashes)
            })
    }
}
