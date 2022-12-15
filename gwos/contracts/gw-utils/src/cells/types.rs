//! Cell types

use crate::gw_common::{CKB_SUDT_SCRIPT_ARGS};
use crate::gw_types::packed::{
    ChallengeLockArgs, CustodianLockArgs, DepositLockArgs, Script, StakeLockArgs,
    WithdrawalLockArgs,
};
use crate::gw_types::h256::H256;

#[derive(PartialEq, Eq, Debug, Clone)]
pub struct CellValue {
    pub sudt_script_hash: H256,
    pub amount: u128,
    pub capacity: u64,
}

impl CellValue {
    pub fn is_ckb_only(&self) -> bool {
        self.sudt_script_hash == CKB_SUDT_SCRIPT_ARGS && self.amount == 0
    }
}

#[derive(Debug)]
pub struct WithdrawalCell {
    pub index: usize,
    pub args: WithdrawalLockArgs,
    pub value: CellValue,
}

#[derive(Clone)]
pub struct DepositRequestCell {
    pub index: usize,
    pub args: DepositLockArgs,
    pub value: CellValue,
    pub account_script: Script,
    pub account_script_hash: H256,
}

#[derive(Debug)]
pub struct CustodianCell {
    pub index: usize,
    pub args: CustodianLockArgs,
    pub value: CellValue,
}

pub struct StakeCell {
    pub index: usize,
    pub args: StakeLockArgs,
    pub capacity: u64,
}

pub struct ChallengeCell {
    pub index: usize,
    pub args: ChallengeLockArgs,
    pub value: CellValue,
}

pub struct BurnCell {
    pub index: usize,
    pub value: CellValue,
}
