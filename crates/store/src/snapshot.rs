use gw_db::schema::Col;
use gw_db::{
    iter::{DBIter, DBIterator, IteratorMode},
    DBPinnableSlice, RocksDBSnapshot,
};

pub struct StoreSnapshot {
    pub(crate) inner: RocksDBSnapshot,
}

impl<'a> StoreSnapshot {
    fn get(&'a self, col: Col, key: &[u8]) -> Option<DBPinnableSlice<'a>> {
        self.inner
            .get_pinned(col, key)
            .expect("db operation should be ok")
    }

    fn get_iter(&self, col: Col, mode: IteratorMode) -> DBIter {
        self.inner
            .iter(col, mode)
            .expect("db operation should be ok")
    }
}
