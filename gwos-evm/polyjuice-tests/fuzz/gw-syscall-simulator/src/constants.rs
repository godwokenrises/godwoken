pub const CREATOR_ACCOUNT_ID: u32 = 1;
pub const CHAIN_ID: u64 = 1;

pub const META_VALIDATOR_SCRIPT_TYPE_HASH: [u8; 32] = [0xa1u8; 32];
pub const SUDT_VALIDATOR_SCRIPT_TYPE_HASH: [u8; 32] = [0xa2u8; 32];
pub const ROLLUP_SCRIPT_HASH: [u8; 32] = [0xa9u8; 32];
pub const ETH_ACCOUNT_LOCK_CODE_HASH: [u8; 32] = [0xaau8; 32];
pub const SECP_LOCK_CODE_HASH: [u8; 32] = [0xbbu8; 32];
pub const POLYJUICE_PROGRAM_CODE_HASH: [u8; 32] = [0xb1u8; 32];
pub const ETH_ADDRESS_REGISTRY_PROGRAM_CODE_HASH: [u8; 32] = [0xb2u8; 32];
pub const BLOCK_HASH: [u8; 32] = [0xc1; 32];
pub const BLOCK_PRODUCER_ETH_ADDRESSS: &str = "a1ad227Ad369f593B5f3d0Cc934A681a50811CB2";

pub const GW_LOG_SUDT_TRANSFER: u8 = 0x0;
pub const GW_LOG_SUDT_PAY_FEE: u8 = 0x1;
pub const GW_LOG_POLYJUICE_SYSTEM: u8 = 0x2;
pub const GW_LOG_POLYJUICE_USER: u8 = 0x3;

pub const SUCCESS: i32 = 0;
pub const ERROR: i32 = -9;
/* Data Fatals */
pub const GW_FATAL_BUFFER_OVERFLOW: i8 = 50;
pub const GW_FATAL_INVALID_CONTEXT: i8 = 51;
pub const GW_FATAL_INVALID_DATA: i8 = 52;
pub const GW_FATAL_MISMATCH_RETURN_DATA: i8 = 53;
pub const GW_FATAL_UNKNOWN_ARGS: i8 = 54;
pub const GW_FATAL_INVALID_SUDT_SCRIPT: i8 = 55;

/* Notfound Fatals */
pub const GW_FATAL_DATA_CELL_NOT_FOUND: i8 = 60;
pub const GW_FATAL_STATE_KEY_NOT_FOUND: i8 = 61;
pub const GW_FATAL_SIGNATURE_CELL_NOT_FOUND: i8 = 62;
pub const GW_FATAL_SCRIPT_NOT_FOUND: i8 = 63;

/* Merkle Fatals */
pub const GW_FATAL_INVALID_PROOF: i8 = 70;
pub const GW_FATAL_INVALID_STACK: i8 = 71;
pub const GW_FATAL_INVALID_SIBLING: i8 = 72;

/* User Errors */
pub const GW_ERROR_DUPLICATED_SCRIPT_HASH: i8 = 80;
pub const GW_ERROR_UNKNOWN_SCRIPT_CODE_HASH: i8 = 81;
pub const GW_ERROR_INVALID_ACCOUNT_SCRIPT: i8 = 82;
pub const GW_ERROR_NOT_FOUND: i8 = 83;
pub const GW_ERROR_RECOVER: i8 = 84;
pub const GW_ERROR_ACCOUNT_NOT_FOUND: i8 = 85;

/* SUDT */
pub const GW_SUDT_ERROR_INSUFFICIENT_BALANCE: i8 = 92i8;
pub const GW_SUDT_ERROR_AMOUNT_OVERFLOW: i8 = 93i8;

pub const GW_ITEM_MISSING: i32 = 1;

// 25KB is max ethereum contract code size
const MAX_SET_RETURN_DATA_SIZE: u64 = 1024 * 25;
