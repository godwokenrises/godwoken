pub mod db;
pub mod error;
pub mod iter;
pub mod memory_stats;
pub mod schema;
pub mod snapshot;
pub mod transaction;
pub mod write_batch;

// re-exports
pub use crate::db::RocksDB;
pub use crate::iter::DBIterator;
pub use crate::memory_stats::CfMemStat;
pub use crate::snapshot::RocksDBSnapshot;
pub use crate::transaction::{RocksDBTransaction, RocksDBTransactionSnapshot};
pub use crate::write_batch::RocksDBWriteBatch;
pub use rocksdb::{
    self as internal, DBPinnableSlice, DBRawIterator, DBVector, Direction, Error as DBError,
    IteratorMode, ReadOptions, WriteBatch,
};

use error::Error;
use std::fmt;
/// The type returned by database methods.
pub type Result<T> = std::result::Result<T, Error>;

fn internal_error<S: fmt::Display>(reason: S) -> Error {
    format!("{}", reason).into()
}
