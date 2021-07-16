#![allow(dead_code)]

pub const SUCCESS: u8 = 0;

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
pub const GW_ERROR_INVALID_CONTRACT_SCRIPT: i8 = 82;
pub const GW_ERROR_NOT_FOUND: i8 = 83;
pub const GW_ERROR_RECOVER: i8 = 84;
pub const GW_ERROR_ACCOUNT_NOT_FOUND: i8 = 85;

/* SUDT */
pub const GW_SUDT_ERROR_INSUFFICIENT_BALANCE: i8 = 92i8;
pub const GW_SUDT_ERROR_AMOUNT_OVERFLOW: i8 = 93i8;
