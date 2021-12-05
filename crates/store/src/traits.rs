use gw_db::error::Error;
use gw_db::schema::Col;
pub trait KVStore {
    fn get(&self, col: Col, key: &[u8]) -> Option<Box<[u8]>>;

    fn insert_raw(&self, col: Col, key: &[u8], value: &[u8]) -> Result<(), Error>;

    fn delete(&self, col: Col, key: &[u8]) -> Result<(), Error>;
}
