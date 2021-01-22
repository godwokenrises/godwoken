use crate::types::{BlockContext, StakeCell};
use crate::{cells::fetch_capacity_and_sudt_value, error::Error};
use gw_common::state::State;
use gw_types::{core::ScriptHashType, packed::{Byte32, L2Block, RollupConfig, StakeLockArgs, StakeLockArgsReader}, prelude::*};
use validator_utils::{ckb_std::{ckb_constants::Source, high_level::{QueryIter, load_cell_data, load_cell_lock}}, search_cells::search_lock_hash};

const REQUIRED_CAPACITY: u64 = 500_00000000u64;

fn parse_stake_lock_args(index: usize, source: Source) -> Result<StakeLockArgs, Error> {
    let data = load_cell_data(index, source)?;
    match StakeLockArgsReader::verify(&data, false) {
        Ok(_) => Ok(StakeLockArgs::new_unchecked(data.into())),
        Err(_) => Err(Error::Encoding),
    }
}

fn find_stake_cell(
    rollup_type_hash: &[u8; 32],
    config: &RollupConfig,
    source: Source,
    owner_lock_hash: &Byte32,
) -> Result<StakeCell, Error> {
    let iter = QueryIter::new(load_cell_lock, source)
        .enumerate()
        .filter_map(|(index, lock)| {
            let is_lock = &lock.args().as_slice()[..32] == rollup_type_hash
                && lock.code_hash().as_slice() == config.stake_type_hash().as_slice()
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
            let cell = StakeCell { index, args, value };
            Some(Ok(cell))
        })
        .take(2);
    // reject if found multiple stake cells
    let mut stake_cell_opt = None;
    for cell in iter {
        if stake_cell_opt.is_some() {
            return Err(Error::Stake);
        }
        stake_cell_opt = Some(cell?);
    }
    let stake_cell = stake_cell_opt.ok_or(Error::Stake)?;
    // check owner_lock_hash
    if &stake_cell.args.owner_lock_hash() != owner_lock_hash {
        return Err(Error::Stake);
    }
    Ok(stake_cell)
}

/// Verify block peroducer
pub fn verify_block_producer(
    config: &RollupConfig,
    context: &BlockContext,
    block: &L2Block,
) -> Result<(), Error> {
    let raw_block = block.raw();
    let owner_lock_hash = raw_block.stake_cell_owner_lock_hash();
    let stake_cell = find_stake_cell(
        &context.rollup_type_hash,
        config,
        Source::Input,
        &owner_lock_hash,
    )?;
    // check stake cell capacity
    if stake_cell.value.capacity < REQUIRED_CAPACITY {
        return Err(Error::Stake);
    }
    // expected output stake args
    let expected_stake_lock_args = stake_cell
        .args
        .as_builder()
        .stake_block_number(raw_block.number())
        .build();
    let output_stake_cell = find_stake_cell(
        &context.rollup_type_hash,
        config,
        Source::Output,
        &owner_lock_hash,
    )?;
    if expected_stake_lock_args != output_stake_cell.args
        || stake_cell.value != output_stake_cell.value
    {
        return Err(Error::Stake);
    }

    Ok(())
}
