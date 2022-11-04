//! state context
//! supports read / write to global state

use gw_common::sparse_merkle_tree::H256;
use gw_utils::gw_common;

#[derive(Clone)]
pub struct DepositRequest {
    // CKB amount
    pub capacity: u64,
    // SUDT amount
    pub amount: u128,
    pub sudt_script_hash: H256,
    pub account_script_hash: H256,
}

#[derive(Clone)]
pub struct WithdrawalRequest {
    pub nonce: u32,
    // CKB amount
    pub capacity: u64,
    // SUDT amount
    pub amount: u128,
    pub sudt_script_hash: H256,
    // layer2 account_script_hash
    pub account_script_hash: H256,
    // Withdrawal request hash
    pub hash: H256,
}
pub struct BlockContext {
    pub number: u64,
    pub finalized_number: u64,
    pub timestamp: u64,
    pub block_hash: H256,
    pub rollup_type_hash: H256,
    pub prev_account_root: H256,
}
