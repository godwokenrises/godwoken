use gw_types::{
    core::Status,
    packed::{GlobalState, RollupConfig},
};
use validator_utils::{ckb_std::ckb_constants::Source, error::Error};

use crate::cells::{
    collect_custodian_locks, collect_deposition_locks, collect_stake_cells,
    collect_withdrawal_locks,
};

pub mod challenge;
pub mod revert;
pub mod submit_block;

/// this function ensure transaction doesn't contains any deposition / withdrawal / custodian
pub fn check_rollup_lock_cells_except_stake(
    rollup_type_hash: &[u8; 32],
    config: &RollupConfig,
) -> Result<(), Error> {
    if !collect_deposition_locks(rollup_type_hash, config, Source::Input)?.is_empty() {
        return Err(Error::InvalidDepositCell);
    }
    if !collect_deposition_locks(rollup_type_hash, config, Source::Output)?.is_empty() {
        return Err(Error::InvalidDepositCell);
    }
    if !collect_withdrawal_locks(rollup_type_hash, config, Source::Input)?.is_empty() {
        return Err(Error::InvalidWithdrawalCell);
    }
    if !collect_withdrawal_locks(rollup_type_hash, config, Source::Output)?.is_empty() {
        return Err(Error::InvalidWithdrawalCell);
    }
    if !collect_custodian_locks(rollup_type_hash, config, Source::Input)?.is_empty() {
        return Err(Error::InvalidCustodianCell);
    }
    if !collect_custodian_locks(rollup_type_hash, config, Source::Output)?.is_empty() {
        return Err(Error::InvalidCustodianCell);
    }
    Ok(())
}

/// this function ensure transaction doesn't contains any deposition / withdrawal / custodian / stake cells
pub fn check_rollup_lock_cells(
    rollup_type_hash: &[u8; 32],
    config: &RollupConfig,
) -> Result<(), Error> {
    check_rollup_lock_cells_except_stake(rollup_type_hash, config)?;
    if !collect_stake_cells(rollup_type_hash, config, Source::Input)?.is_empty() {
        return Err(Error::InvalidStakeCell);
    }
    if !collect_stake_cells(rollup_type_hash, config, Source::Output)?.is_empty() {
        return Err(Error::InvalidStakeCell);
    }
    Ok(())
}

pub fn check_status(global_state: &GlobalState, status: Status) -> Result<(), Error> {
    let expected_status: u8 = status.into();
    let status: u8 = global_state.status().into();
    if status != expected_status {
        return Err(Error::InvalidStatus);
    }
    Ok(())
}
