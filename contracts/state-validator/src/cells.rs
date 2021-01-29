//! Cells operations

use crate::types::CellValue;
use crate::{
    ckb_std::ckb_types::prelude::Entity as CKBEntity,
    types::{
        BurnCell, ChallengeCell, CustodianCell, DepositionRequestCell, StakeCell, WithdrawalCell,
    },
};
use alloc::vec::Vec;
use gw_common::{CKB_SUDT_SCRIPT_ARGS, H256};
use gw_types::{
    bytes::Bytes,
    core::ScriptHashType,
    packed::{
        Byte32, ChallengeLockArgs, ChallengeLockArgsReader, CustodianLockArgs,
        CustodianLockArgsReader, DepositionLockArgs, DepositionLockArgsReader, GlobalState,
        GlobalStateReader, RollupConfig, Script, StakeLockArgs, StakeLockArgsReader,
        WithdrawalLockArgs, WithdrawalLockArgsReader,
    },
    prelude::*,
};
use validator_utils::{
    ckb_std::{
        ckb_constants::Source,
        high_level::{
            load_cell_capacity, load_cell_data, load_cell_lock, load_cell_lock_hash,
            load_cell_type, load_cell_type_hash, QueryIter,
        },
    },
    error::Error,
};

fn fetch_sudt_script_hash(
    config: &RollupConfig,
    index: usize,
    source: Source,
) -> Result<Option<[u8; 32]>, Error> {
    match load_cell_type(index, source)? {
        Some(type_) => {
            if type_.hash_type() == ScriptHashType::Type.into()
                && type_.code_hash().as_slice() == config.l1_sudt_script_type_hash().as_slice()
            {
                return Ok(load_cell_type_hash(index, source)?);
            }
            Err(Error::InvalidSUDTCell)
        }
        None => Ok(None),
    }
}

/// fetch capacity and SUDT value of a cell
pub fn fetch_capacity_and_sudt_value(
    config: &RollupConfig,
    index: usize,
    source: Source,
) -> Result<CellValue, Error> {
    let capacity = load_cell_capacity(index, source)?;
    let value = match fetch_sudt_script_hash(config, index, source)? {
        Some(sudt_script_hash) => {
            let data = load_cell_data(index, source)?;
            let mut buf = [0u8; 16];
            buf.copy_from_slice(&data[..16]);
            let amount = u128::from_le_bytes(buf);
            CellValue {
                sudt_script_hash: sudt_script_hash.into(),
                amount,
                capacity,
            }
        }
        None => CellValue {
            sudt_script_hash: H256::zero(),
            amount: 0,
            capacity,
        },
    };
    Ok(value)
}

pub fn parse_global_state(source: Source) -> Result<GlobalState, Error> {
    let data = load_cell_data(0, source)?;
    match GlobalStateReader::verify(&data, false) {
        Ok(_) => Ok(GlobalState::new_unchecked(data.into())),
        Err(_) => Err(Error::Encoding),
    }
}

pub fn collect_stake_cells(
    rollup_type_hash: &[u8; 32],
    config: &RollupConfig,
    source: Source,
) -> Result<Vec<StakeCell>, Error> {
    let iter = QueryIter::new(load_cell_lock, source)
        .enumerate()
        .filter_map(|(index, lock)| {
            let is_lock = &lock.args().as_slice()[..32] == rollup_type_hash
                && lock.code_hash().as_slice() == config.stake_script_type_hash().as_slice()
                && lock.hash_type() == ScriptHashType::Type.into();
            if !is_lock {
                return None;
            }
            let raw_args = lock.args().as_slice()[32..].to_vec();
            let args = match StakeLockArgsReader::verify(&raw_args, false) {
                Ok(_) => StakeLockArgs::new_unchecked(raw_args.into()),
                Err(_) => {
                    return Some(Err(Error::Encoding));
                }
            };
            let value = match fetch_capacity_and_sudt_value(config, index, Source::Input) {
                Ok(value) => value,
                Err(err) => return Some(Err(err)),
            };
            // we only accept CKB as staking assets for now
            if value.sudt_script_hash != CKB_SUDT_SCRIPT_ARGS.into() || value.amount != 0 {
                return Some(Err(Error::InvalidStakeCell));
            }
            let cell = StakeCell { index, args, value };
            Some(Ok(cell))
        });
    // reject if found multiple stake cells
    let cells = iter.collect::<Result<Vec<_>, Error>>()?;
    Ok(cells)
}

/// Find one stake cell
/// this function ensure we have only 1 stake cell in the source
/// and the cell's owner_lock_hash must matches the owner_lock_hash arg
pub fn find_one_stake_cell(
    rollup_type_hash: &[u8; 32],
    config: &RollupConfig,
    source: Source,
    owner_lock_hash: &Byte32,
) -> Result<StakeCell, Error> {
    let mut cells = collect_stake_cells(rollup_type_hash, config, source)?;
    // this function guratee only one cell in the source
    if cells.len() != 1 {
        return Err(Error::InvalidStakeCell);
    }
    if &cells[0].args.owner_lock_hash() != owner_lock_hash {
        return Err(Error::InvalidStakeCell);
    }
    Ok(cells.remove(0))
}

pub fn find_challenge_cell(
    rollup_type_hash: &[u8; 32],
    config: &RollupConfig,
    source: Source,
) -> Result<Option<ChallengeCell>, Error> {
    let iter = QueryIter::new(load_cell_lock, source)
        .enumerate()
        .filter_map(|(index, lock)| {
            let is_lock = &lock.args().as_slice()[..32] == rollup_type_hash
                && lock.code_hash().as_slice() == config.challenge_script_type_hash().as_slice()
                && lock.hash_type() == ScriptHashType::Type.into();
            if !is_lock {
                return None;
            }
            let raw_args = lock.args().as_slice()[32..].to_vec();
            let args = match ChallengeLockArgsReader::verify(&raw_args, false) {
                Ok(_) => ChallengeLockArgs::new_unchecked(raw_args.into()),
                Err(_) => {
                    return Some(Err(Error::Encoding));
                }
            };
            let value = match fetch_capacity_and_sudt_value(config, index, source) {
                Ok(value) => value,
                Err(err) => {
                    return Some(Err(err));
                }
            };
            if value.sudt_script_hash != CKB_SUDT_SCRIPT_ARGS.into() || value.amount != 0 {
                return None;
            }
            let cell = ChallengeCell { index, args, value };
            Some(Ok(cell))
        })
        .take(2);
    // reject if found multiple stake cells
    let mut cells = iter.collect::<Result<Vec<_>, Error>>()?;
    if cells.len() > 1 {
        return Err(Error::InvalidChallengeCell);
    }
    Ok(cells.pop())
}

pub fn build_l2_sudt_script(config: &RollupConfig, l1_sudt_script_hash: [u8; 32]) -> Script {
    let args = Bytes::from(l1_sudt_script_hash.to_vec());
    Script::new_builder()
        .args(args.pack())
        .code_hash(config.l2_sudt_validator_script_type_hash())
        .hash_type(ScriptHashType::Type.into())
        .build()
}

pub fn collect_withdrawal_locks(
    rollup_type_hash: &[u8; 32],
    config: &RollupConfig,
    source: Source,
) -> Result<Vec<WithdrawalCell>, Error> {
    QueryIter::new(load_cell_lock, source)
        .enumerate()
        .filter_map(|(index, lock)| {
            let is_withdrawal_lock = &lock.args().as_slice()[..32] == rollup_type_hash
                && lock.code_hash().as_slice() == config.withdrawal_script_type_hash().as_slice()
                && lock.hash_type() == ScriptHashType::Type.into();
            if !is_withdrawal_lock {
                return None;
            }
            let raw_args = lock.args().as_slice()[32..].to_vec();
            let args = match WithdrawalLockArgsReader::verify(&raw_args, false) {
                Ok(_) => WithdrawalLockArgs::new_unchecked(raw_args.into()),
                Err(_) => {
                    return Some(Err(Error::Encoding));
                }
            };
            let value = match fetch_capacity_and_sudt_value(config, index, Source::Output) {
                Ok(value) => value,
                Err(err) => return Some(Err(err)),
            };
            Some(Ok(WithdrawalCell { index, args, value }))
        })
        .collect::<Result<_, Error>>()
}

pub fn collect_custodian_locks(
    rollup_type_hash: &[u8; 32],
    config: &RollupConfig,
    source: Source,
) -> Result<Vec<CustodianCell>, Error> {
    QueryIter::new(load_cell_lock, source)
        .enumerate()
        .filter_map(|(index, lock)| {
            let is_lock = &lock.args().as_slice()[..32] == rollup_type_hash
                && lock.code_hash().as_slice() == config.custodian_script_type_hash().as_slice()
                && lock.hash_type() == ScriptHashType::Type.into();
            if !is_lock {
                return None;
            }
            let raw_args = lock.args().as_slice()[32..].to_vec();
            let args = match CustodianLockArgsReader::verify(&raw_args, false) {
                Ok(_) => CustodianLockArgs::new_unchecked(raw_args.into()),
                Err(_) => {
                    return Some(Err(Error::Encoding));
                }
            };
            let value = match fetch_capacity_and_sudt_value(config, index, Source::Input) {
                Ok(value) => value,
                Err(err) => return Some(Err(err)),
            };
            let cell = CustodianCell { index, args, value };
            Some(Ok(cell))
        })
        .collect::<Result<_, Error>>()
}

pub fn collect_deposition_locks(
    rollup_type_hash: &[u8; 32],
    config: &RollupConfig,
    source: Source,
) -> Result<Vec<DepositionRequestCell>, Error> {
    QueryIter::new(load_cell_lock, source)
        .enumerate()
        .filter_map(|(index, lock)| {
            let is_lock = &lock.args().as_slice()[..32] == rollup_type_hash
                && lock.code_hash().as_slice() == config.deposition_script_type_hash().as_slice()
                && lock.hash_type() == ScriptHashType::Type.into();
            if !is_lock {
                return None;
            }
            let raw_args = lock.args().as_slice()[32..].to_vec();
            let args = match DepositionLockArgsReader::verify(&raw_args, false) {
                Ok(_) => DepositionLockArgs::new_unchecked(raw_args.into()),
                Err(_) => {
                    return Some(Err(Error::Encoding));
                }
            };
            let account_script_hash = args.layer2_lock().hash().into();
            let value = match fetch_capacity_and_sudt_value(config, index, Source::Input) {
                Ok(value) => value,
                Err(err) => return Some(Err(err)),
            };
            let cell = DepositionRequestCell {
                index,
                args,
                value,
                account_script_hash,
            };
            Some(Ok(cell))
        })
        .collect::<Result<_, Error>>()
}

pub fn collect_burn_cells(config: &RollupConfig, source: Source) -> Result<Vec<BurnCell>, Error> {
    QueryIter::new(load_cell_lock_hash, source)
        .enumerate()
        .filter_map(|(index, lock_hash)| {
            let is_lock = &lock_hash == config.burn_lock_hash().as_slice();
            if !is_lock {
                return None;
            }
            let value = match fetch_capacity_and_sudt_value(config, index, Source::Input) {
                Ok(value) => value,
                Err(err) => return Some(Err(err)),
            };
            let cell = BurnCell { index, value };
            Some(Ok(cell))
        })
        .collect::<Result<_, Error>>()
}
