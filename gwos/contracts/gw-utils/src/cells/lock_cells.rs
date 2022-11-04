//! Lock cells

use super::types::{
    BurnCell, CellValue, ChallengeCell, CustodianCell, DepositRequestCell, StakeCell,
    WithdrawalCell,
};
use crate::error::Error;
use alloc::vec::Vec;
use ckb_std::{
    ckb_constants::Source,
    ckb_types::prelude::{Entity as CKBEntity, Unpack as CKBUnpack},
    debug,
    high_level::{
        load_cell_capacity, load_cell_data, load_cell_lock, load_cell_lock_hash, load_cell_type,
        load_cell_type_hash, QueryIter,
    },
};
use gw_common::{CKB_SUDT_SCRIPT_ARGS, H256};
use gw_types::{
    bytes::Bytes,
    core::ScriptHashType,
    packed::{Byte32, Byte32Reader, DepositLockArgs, RollupConfig, StakeLockArgs},
    prelude::*,
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

/// used in filter_map
fn extract_args_from_lock<ArgsType: Entity>(
    lock: &crate::ckb_std::ckb_types::packed::Script,
    rollup_type_hash: &H256,
    lock_script_type_hash: &Byte32,
) -> Option<Result<ArgsType, Error>> {
    let lock_args: Bytes = lock.args().unpack();
    let is_lock = lock_args.len() > 32
        && &lock_args[..32] == rollup_type_hash.as_slice()
        && lock.code_hash().as_slice() == lock_script_type_hash.as_slice()
        && lock.hash_type() == ScriptHashType::Type.into();

    // return none to skip this cell
    if !is_lock {
        return None;
    }

    // parse the remaining lock_args
    let raw_args = lock_args[32..].to_vec();
    Some(ArgsType::from_slice(&raw_args).map_err(|_err| {
        debug!("Fail to extract args, lock args parsing err");
        Error::Encoding
    }))
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

pub fn collect_stake_cells(
    rollup_type_hash: &H256,
    config: &RollupConfig,
    source: Source,
) -> Result<Vec<StakeCell>, Error> {
    let iter = QueryIter::new(load_cell_lock, source)
        .enumerate()
        .filter_map(|(index, lock)| -> Option<Result<StakeCell, _>> {
            let args = match extract_args_from_lock::<StakeLockArgs>(
                &lock,
                rollup_type_hash,
                &config.stake_script_type_hash(),
            ) {
                Some(Ok(args)) => args,
                Some(Err(err)) => return Some(Err(err)),
                None => return None,
            };
            let value = match fetch_capacity_and_sudt_value(config, index, source) {
                Ok(value) => value,
                Err(err) => return Some(Err(err)),
            };
            // we only accept CKB as staking assets for now
            if value.sudt_script_hash != CKB_SUDT_SCRIPT_ARGS.into() || value.amount != 0 {
                debug!("found a stake cell with simple UDT");
                return Some(Err(Error::InvalidStakeCell));
            }
            let cell = StakeCell {
                index,
                args,
                capacity: value.capacity,
            };
            Some(Ok(cell))
        });
    // reject if found multiple stake cells
    let cells = iter.collect::<Result<Vec<_>, Error>>()?;
    Ok(cells)
}

/// Find block producer's stake cell
/// this function return Option<StakeCell> if we have 1 or zero stake cell,
/// otherwise return an error.
pub fn find_block_producer_stake_cell(
    rollup_type_hash: &H256,
    config: &RollupConfig,
    source: Source,
    owner_lock_hash: &Byte32Reader,
) -> Result<Option<StakeCell>, Error> {
    let mut cells = collect_stake_cells(rollup_type_hash, config, source)?;
    // return an error if more than one stake cell returned
    if cells.len() > 1 {
        debug!(
            "expected no more than 1 stake cell from {:?}, found {}",
            source,
            cells.len()
        );
        return Err(Error::InvalidStakeCell);
    }
    if cells
        .iter()
        .any(|cell| cell.args.owner_lock_hash().as_slice() != owner_lock_hash.as_slice())
    {
        debug!("found stake cell with unexpected owner_lock_hash");
        return Err(Error::InvalidStakeCell);
    }
    Ok(cells.pop())
}

pub fn find_challenge_cell(
    rollup_type_hash: &H256,
    config: &RollupConfig,
    source: Source,
) -> Result<Option<ChallengeCell>, Error> {
    let iter = QueryIter::new(load_cell_lock, source)
        .enumerate()
        .filter_map(|(index, lock)| {
            let args = match extract_args_from_lock(
                &lock,
                rollup_type_hash,
                &config.challenge_script_type_hash(),
            ) {
                Some(Ok(args)) => args,
                Some(Err(err)) => return Some(Err(err)),
                None => return None,
            };
            let value = match fetch_capacity_and_sudt_value(config, index, source) {
                Ok(value) => value,
                Err(err) => {
                    return Some(Err(err));
                }
            };
            if value.sudt_script_hash != CKB_SUDT_SCRIPT_ARGS.into() || value.amount != 0 {
                debug!("found a challenge cell with simple UDT");
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

pub fn collect_withdrawal_locks(
    rollup_type_hash: &H256,
    config: &RollupConfig,
    source: Source,
) -> Result<Vec<WithdrawalCell>, Error> {
    QueryIter::new(load_cell_lock, source)
        .enumerate()
        .filter_map(|(index, lock)| {
            let lock_args: Bytes = lock.args().unpack();
            let is_withdrawal_lock = lock_args.len() > 32
                && &lock_args[..32] == rollup_type_hash.as_slice()
                && lock.code_hash().as_slice() == config.withdrawal_script_type_hash().as_slice()
                && lock.hash_type() == ScriptHashType::Type.into();
            if !is_withdrawal_lock {
                return None;
            }
            let args = match crate::withdrawal::parse_lock_args(&lock_args) {
                Ok(r) => r.lock_args,
                Err(_) => {
                    debug!("Fail to parsing withdrawal lock args");
                    return Some(Err(Error::Encoding));
                }
            };

            let value = match fetch_capacity_and_sudt_value(config, index, source) {
                Ok(value) => value,
                Err(err) => return Some(Err(err)),
            };
            Some(Ok(WithdrawalCell { index, args, value }))
        })
        .collect::<Result<_, Error>>()
}

pub fn collect_custodian_locks(
    rollup_type_hash: &H256,
    config: &RollupConfig,
    source: Source,
) -> Result<Vec<CustodianCell>, Error> {
    QueryIter::new(load_cell_lock, source)
        .enumerate()
        .filter_map(|(index, lock)| {
            let args = match extract_args_from_lock(
                &lock,
                rollup_type_hash,
                &config.custodian_script_type_hash(),
            ) {
                Some(Ok(args)) => args,
                Some(Err(err)) => return Some(Err(err)),
                None => return None,
            };
            let value = match fetch_capacity_and_sudt_value(config, index, source) {
                Ok(value) => value,
                Err(err) => return Some(Err(err)),
            };
            let cell = CustodianCell { index, args, value };
            Some(Ok(cell))
        })
        .collect::<Result<_, Error>>()
}

pub fn collect_deposit_locks(
    rollup_type_hash: &H256,
    config: &RollupConfig,
    source: Source,
) -> Result<Vec<DepositRequestCell>, Error> {
    QueryIter::new(load_cell_lock, source)
        .enumerate()
        .filter_map(|(index, lock)| {
            let args: DepositLockArgs = match extract_args_from_lock(
                &lock,
                rollup_type_hash,
                &config.deposit_script_type_hash(),
            ) {
                Some(Ok(args)) => args,
                Some(Err(err)) => return Some(Err(err)),
                None => return None,
            };
            let value = match fetch_capacity_and_sudt_value(config, index, source) {
                Ok(value) => value,
                Err(err) => return Some(Err(err)),
            };
            let account_script = args.layer2_lock();
            let account_script_hash = account_script.hash().into();
            let cell = DepositRequestCell {
                index,
                args,
                value,
                account_script,
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
            let is_lock = lock_hash == config.burn_lock_hash().as_slice();
            if !is_lock {
                return None;
            }
            let value = match fetch_capacity_and_sudt_value(config, index, source) {
                Ok(value) => value,
                Err(err) => return Some(Err(err)),
            };
            let cell = BurnCell { index, value };
            Some(Ok(cell))
        })
        .collect::<Result<_, Error>>()
}
