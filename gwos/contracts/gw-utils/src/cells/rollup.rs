use ckb_std::{
    ckb_constants::Source,
    debug,
    high_level::{load_cell_data, load_cell_data_hash, load_cell_type_hash, QueryIter},
    syscalls::{load_witness, SysError},
};
use gw_types::{
    packed::{
        GlobalState, GlobalStateReader, GlobalStateV0, GlobalStateV0Reader, RollupActionReader,
        RollupConfig, RollupConfigReader, WitnessArgsReader,
    },
    prelude::*,
};

use crate::error::Error;

/// 524_288 we choose this value because it is smaller than the MAX_BLOCK_BYTES which is 597K
pub const MAX_ROLLUP_WITNESS_SIZE: usize = 1 << 19;

pub fn search_rollup_cell(rollup_type_hash: &[u8; 32], source: Source) -> Option<usize> {
    QueryIter::new(load_cell_type_hash, source)
        .position(|type_hash| type_hash.as_ref() == Some(rollup_type_hash))
}

fn search_rollup_config_cell(rollup_config_hash: &[u8; 32]) -> Option<usize> {
    QueryIter::new(load_cell_data_hash, Source::CellDep)
        .position(|data_hash| data_hash.as_ref() == rollup_config_hash)
}

pub fn load_rollup_config(rollup_config_hash: &[u8; 32]) -> Result<RollupConfig, Error> {
    let index = search_rollup_config_cell(rollup_config_hash).ok_or(Error::RollupConfigNotFound)?;
    let data = load_cell_data(index, Source::CellDep)?;
    match RollupConfigReader::verify(&data, false) {
        Ok(_) => Ok(RollupConfig::new_unchecked(data.into())),
        Err(_) => {
            debug!("Invalid encoding of RollupConfig");
            Err(Error::Encoding)
        }
    }
}

pub fn search_rollup_state(
    rollup_type_hash: &[u8; 32],
    source: Source,
) -> Result<Option<GlobalState>, SysError> {
    let index = match QueryIter::new(load_cell_type_hash, source)
        .position(|type_hash| type_hash.as_ref() == Some(rollup_type_hash))
    {
        Some(i) => i,
        None => return Ok(None),
    };
    let data = load_cell_data(index, source)?;
    match GlobalStateReader::verify(&data, false) {
        Ok(_) => Ok(Some(GlobalState::new_unchecked(data.into()))),
        Err(_) if GlobalStateV0Reader::verify(&data, false).is_ok() => {
            let global_state_v0 = GlobalStateV0::new_unchecked(data.into());
            Ok(Some(GlobalState::from(global_state_v0)))
        }
        Err(_) => {
            debug!("Invalid encoding of Global state");
            Err(SysError::Encoding)
        }
    }
}

pub fn parse_rollup_action(
    buf: &mut [u8; MAX_ROLLUP_WITNESS_SIZE],
    index: usize,
    source: Source,
) -> Result<RollupActionReader, Error> {
    let loaded_len = load_witness(buf, 0, index, source)?;
    debug!("load rollup witness, loaded len: {}", loaded_len);

    let witness_args = WitnessArgsReader::from_slice(&buf[..loaded_len]).map_err(|_err| {
        debug!("witness is not a valid WitnessArgsReader");
        Error::Encoding
    })?;
    let output = witness_args.output_type().to_opt().ok_or_else(|| {
        debug!("WitnessArgs#output_type is none");
        Error::Encoding
    })?;
    let action = RollupActionReader::from_slice(output.raw_data()).map_err(|_err| {
        debug!("output is not a valid RollupActionReader");
        Error::Encoding
    })?;
    Ok(action)
}
