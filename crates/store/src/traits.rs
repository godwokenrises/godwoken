use crate::db_utils::build_transaction_key;
use gw_common::{
    error::Error as StateError,
    smt::SMT,
    sparse_merkle_tree::{
        error::Error as SMTError,
        traits::Store as SMTStore,
        tree::{BranchNode, LeafNode},
    },
    state::State,
    H256,
};
use gw_db::schema::{
    Col, COLUMN_ACCOUNT_SMT_BRANCH, COLUMN_ACCOUNT_SMT_LEAF, COLUMN_BLOCK,
    COLUMN_BLOCK_DEPOSITION_REQUESTS, COLUMN_BLOCK_GLOBAL_STATE, COLUMN_BLOCK_SMT_BRANCH,
    COLUMN_BLOCK_SMT_LEAF, COLUMN_DATA, COLUMN_INDEX, COLUMN_META, COLUMN_SCRIPT,
    COLUMN_SYNC_BLOCK_HEADER_INFO, COLUMN_TRANSACTION, COLUMN_TRANSACTION_INFO,
    COLUMN_TRANSACTION_RECEIPT, META_ACCOUNT_SMT_COUNT_KEY, META_ACCOUNT_SMT_ROOT_KEY,
    META_BLOCK_SMT_ROOT_KEY, META_CHAIN_ID_KEY, META_TIP_BLOCK_HASH_KEY,
};
use gw_db::{
    error::Error, iter::DBIter, DBIterator, DBVector, IteratorMode, RocksDBTransaction,
    RocksDBTransactionSnapshot,
};
use gw_traits::{ChainStore, CodeStore};
use gw_types::{bytes::Bytes, packed, prelude::*};
pub trait KVStore {
    fn get(&self, col: Col, key: &[u8]) -> Option<DBVector>;

    fn get_iter(&self, col: Col, mode: IteratorMode) -> DBIter;

    fn insert_raw(&self, col: Col, key: &[u8], value: &[u8]) -> Result<(), Error>;

    fn delete(&self, col: Col, key: &[u8]) -> Result<(), Error>;
}
