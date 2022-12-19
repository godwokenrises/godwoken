//! state context
//! supports read / write to global state

use gw_utils::fork::Fork;
use gw_utils::gw_types::core::{Timepoint, H256};

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
    pub timestamp: u64,
    pub block_hash: H256,
    pub rollup_type_hash: H256,
    pub prev_account_root: H256,

    // global_state.version
    pub post_version: u8,

    // finality_time_in_ms(rollup_config)
    pub finality_time_in_ms: u64,
}

impl BlockContext {
    pub const fn finalized_timepoint(&self) -> Timepoint {
        if Fork::use_timestamp_as_timepoint(self.post_version) {
            // the future finalized timestamp of block
            Timepoint::from_timestamp(self.timestamp + self.finality_time_in_ms)
        } else {
            // the current block number
            Timepoint::from_block_number(self.number)
        }
    }
}
