//! The schema include constants define the low level database column families.

/// Column families alias type
pub type Col = u8;
/// Total column number
pub const COLUMNS: u32 = 37;
/// Column store meta data
pub const COLUMN_META: Col = 0;
/// Column store chain index
pub const COLUMN_INDEX: Col = 1;
/// Column store block
pub const COLUMN_BLOCK: Col = 2;
/// Column store block's global state
pub const COLUMN_BLOCK_GLOBAL_STATE: Col = 4;
/// Column store transaction
pub const COLUMN_TRANSACTION: Col = 5;
/// Column store transaction receipt
pub const COLUMN_TRANSACTION_RECEIPT: Col = 6;
/// Column store l2 block committed info. No longer used.
pub const REMOVED_COLUMN_L2BLOCK_COMMITTED_INFO: Col = 7;
/// Column store transaction extra information
pub const COLUMN_TRANSACTION_INFO: Col = 8;
/// Column account SMT branch
pub const COLUMN_ACCOUNT_SMT_BRANCH: Col = 9;
/// Column account SMT leaf
pub const COLUMN_ACCOUNT_SMT_LEAF: Col = 10;
/// Column block SMT branch
pub const COLUMN_BLOCK_SMT_BRANCH: Col = 11;
/// Column block SMT leaf
pub const COLUMN_BLOCK_SMT_LEAF: Col = 12;
/// Column script
pub const COLUMN_SCRIPT: Col = 14;
/// Column data
pub const COLUMN_DATA: Col = 15;
/// Column block deposit requests
pub const REMOVED_COLUMN_BLOCK_DEPOSIT_REQUESTS: Col = 16;
/// Column mem pool transaction
pub const COLUMN_MEM_POOL_TRANSACTION_RECEIPT: Col = 17;
/// Column block state record
pub const COLUMN_BLOCK_STATE_RECORD: Col = 18;
/// Column block state reverse record
pub const COLUMN_BLOCK_STATE_REVERSE_RECORD: Col = 20;
/// Column reverted block SMT branch
pub const COLUMN_REVERTED_BLOCK_SMT_BRANCH: Col = 21;
/// Column reverted block SMT leaf
pub const COLUMN_REVERTED_BLOCK_SMT_LEAF: Col = 22;
/// Column bad block challenge target
pub const COLUMN_BAD_BLOCK_CHALLENGE_TARGET: Col = 23;
/// Column reverted block smt root => reverted block hashes
pub const COLUMN_REVERTED_BLOCK_SMT_ROOT: Col = 24;
/// Column asset script
pub const COLUMN_ASSET_SCRIPT: Col = 25;
/// Column mem pool transaction
pub const COLUMN_MEM_POOL_TRANSACTION: Col = 26;
/// Column mem pool withdrawal
pub const COLUMN_MEM_POOL_WITHDRAWAL: Col = 27;
/// Column store withdrawal
pub const COLUMN_WITHDRAWAL: Col = 33;
/// Column store withdrawal extra information
pub const COLUMN_WITHDRAWAL_INFO: Col = 34;
/// block number (in big endian for natural ordering) -> block submission tx.
///
/// May not be available for all blocks.
pub const COLUMN_BLOCK_SUBMIT_TX: Col = 35;
// Reuse removed column.
/// Block number (in big endian) -> block submission tx hash.
pub const COLUMN_BLOCK_SUBMIT_TX_HASH: Col = 7;
// Reuse removed column.
/// block number (in big endian) -> deposit info vec.
pub const COLUMN_BLOCK_DEPOSIT_INFO_VEC: Col = 16;
/// block number (in big endian) -> FinalizedCustodianCapacity.
pub const COLUMN_BLOCK_POST_FINALIZED_CUSTODIAN_CAPACITY: Col = 36;

/// chain id
pub const META_CHAIN_ID_KEY: &[u8] = b"CHAIN_ID";
/// META_TIP_BLOCK_HASH_KEY tracks the latest known best block hash
pub const META_TIP_BLOCK_HASH_KEY: &[u8] = b"TIP_BLOCK_HASH";
/// block SMT root
pub const META_BLOCK_SMT_ROOT_KEY: &[u8] = b"BLOCK_SMT_ROOT_KEY";
/// reverted block SMT root
pub const META_REVERTED_BLOCK_SMT_ROOT_KEY: &[u8] = b"REVERTED_BLOCK_SMT_ROOT_KEY";
/// track the latest known valid block hash
pub const META_LAST_VALID_TIP_BLOCK_HASH_KEY: &[u8] = b"LAST_VALID_TIP_BLOCK_HASH";
/// track the last known L2 block NumberAndHash confirmed by L1
pub const META_LAST_CONFIRMED_BLOCK_NUMBER_HASH_KEY: &[u8] = b"LAST_CONFIRMED_BLOCK_NUMBER";
/// track the last submitted l2 block NumberAndHash
pub const META_LAST_SUBMITTED_BLOCK_NUMBER_HASH_KEY: &[u8] = b"LAST_SUBMITTED_BLOCK_NUMBER";
/// track last finalized withdrawal tx hash
pub const META_LAST_FINALIZED_WITHDRAWAL_TX_HASH_KEY: &[u8] = b"LAST_FINALIZED_WITHDRAWAL_TX_HASH";

/// CHAIN_SPEC_HASH_KEY tracks the hash of chain spec which created current database
pub const CHAIN_SPEC_HASH_KEY: &[u8] = b"chain-spec-hash";
/// CHAIN_SPEC_HASH_KEY tracks the current database version.
pub const MIGRATION_VERSION_KEY: &[u8] = b"db-version";
