use gw_db::schema::Col;
use gw_db::ReadOptions;
use gw_db::{error::Error, iter::DBIter, IteratorMode};
pub trait KVStore {
    fn get(&self, col: Col, key: &[u8]) -> Option<Box<[u8]>>;

    fn get_iter(&self, col: Col, mode: IteratorMode) -> DBIter;

    fn get_iter_opts(&self, col: Col, mode: IteratorMode, opts: &ReadOptions) -> DBIter;

    fn insert_raw(&self, col: Col, key: &[u8], value: &[u8]) -> Result<(), Error>;

    fn delete(&self, col: Col, key: &[u8]) -> Result<(), Error>;
}
