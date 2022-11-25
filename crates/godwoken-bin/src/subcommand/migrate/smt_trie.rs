use gw_db::{migrate::Migration, Result, RocksDB};

pub struct SMTTrieMigration;

impl Migration for SMTTrieMigration {
    fn migrate(&self, db: RocksDB) -> Result<RocksDB> {
        todo!()
    }
    fn version(&self) -> &str {
        "20221125"
    }
}
