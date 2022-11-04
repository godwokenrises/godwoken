use gw_common::H256;
use gw_types::{
    core::Status,
    packed::{GlobalState, RollupConfig},
};
use gw_utils::gw_common;
use gw_utils::gw_types;
use gw_utils::{
    cells::lock_cells::{
        collect_custodian_locks, collect_deposit_locks, collect_stake_cells,
        collect_withdrawal_locks,
    },
    ckb_std::{ckb_constants::Source, debug},
    error::Error,
};

pub mod challenge;
pub mod revert;
pub mod submit_block;

/// this function ensure transaction doesn't contains any deposit / withdrawal / custodian
pub fn check_rollup_lock_cells_except_stake(
    rollup_type_hash: &H256,
    config: &RollupConfig,
) -> Result<(), Error> {
    if !collect_deposit_locks(rollup_type_hash, config, Source::Input)?.is_empty() {
        return Err(Error::InvalidDepositCell);
    }
    if !collect_deposit_locks(rollup_type_hash, config, Source::Output)?.is_empty() {
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

/// this function ensure transaction doesn't contains any deposit / withdrawal / custodian / stake cells
pub fn check_rollup_lock_cells(
    rollup_type_hash: &H256,
    config: &RollupConfig,
) -> Result<(), Error> {
    check_rollup_lock_cells_except_stake(rollup_type_hash, config)?;
    if !collect_stake_cells(rollup_type_hash, config, Source::Input)?.is_empty() {
        debug!("unexpected input stake cell");
        return Err(Error::InvalidStakeCell);
    }
    if !collect_stake_cells(rollup_type_hash, config, Source::Output)?.is_empty() {
        debug!("unexpected output stake cell");
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
