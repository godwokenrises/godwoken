//! The schema include constants define the low level database column families.

/// Column families alias type
pub type Col = &'static str;
/// Total column number
pub const COLUMNS: u32 = 15;
/// Column store meta data
pub const COLUMN_META: Col = "0";
/// Column store chain index
pub const COLUMN_INDEX: Col = "1";
/// Column store block
pub const COLUMN_BLOCK: Col = "2";
/// Column store block's header info
pub const COLUMN_BLOCK_HEADER_INFO: Col = "3";
/// Column store block's global state
pub const COLUMN_BLOCK_GLOBAL_STATE: Col = "4";
/// Column store transaction
pub const COLUMN_TRANSACTION: Col = "5";
/// Column store transaction receipt
pub const COLUMN_TRANSACTION_RECEIPT: Col = "6";
/// Column store sync block header info
pub const COLUMN_SYNC_BLOCK_HEADER_INFO: Col = "7";
/// Column store transaction extra information
pub const COLUMN_TRANSACTION_INFO: Col = "8";
/// Column account SMT branch
pub const COLUMN_ACCOUNT_SMT_BRANCH: Col = "9";
/// Column account SMT leaf
pub const COLUMN_ACCOUNT_SMT_LEAF: Col = "10";
/// Column block SMT branch
pub const COLUMN_BLOCK_SMT_BRANCH: Col = "11";
/// Column block SMT leaf
pub const COLUMN_BLOCK_SMT_LEAF: Col = "12";
/// Column store block number-hash pair
pub const COLUMN_NUMBER_HASH: Col = "13";
/// Column script
pub const COLUMN_SCRIPT: Col = "14";
/// Column data
pub const COLUMN_DATA: Col = "15";

/// META_TIP_BLOCK_HASH_KEY tracks the latest known best block hash
pub const META_TIP_BLOCK_HASH_KEY: &[u8] = b"TIP_BLOCK_HASH";
/// block SMT root
pub const META_BLOCK_SMT_ROOT_KEY: &[u8] = b"BLOCK_SMT_ROOT_KEY";
/// account SMT root
pub const META_ACCOUNT_SMT_ROOT_KEY: &[u8] = b"ACCOUNT_SMT_ROOT_KEY";
/// account SMT count
pub const META_ACCOUNT_SMT_COUNT_KEY: &[u8] = b"ACCOUNT_SMT_COUNT_KEY";

/// CHAIN_SPEC_HASH_KEY tracks the hash of chain spec which created current database
pub const CHAIN_SPEC_HASH_KEY: &[u8] = b"chain-spec-hash";
/// CHAIN_SPEC_HASH_KEY tracks the current database version.
pub const MIGRATION_VERSION_KEY: &[u8] = b"db-version";
