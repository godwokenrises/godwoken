//! Cells operations

use crate::ckb_std::ckb_types::prelude::{Entity as CKBEntity, Unpack as CKBUnpack};
use gw_common::H256;
use gw_types::{core::ScriptHashType, packed::RollupConfig};
use validator_utils::ckb_std::{
    ckb_constants::Source,
    high_level::{load_cell_capacity, load_cell_data, load_cell_type, load_cell_type_hash},
};

use crate::{error::Error, types::CellValue};

fn fetch_sudt_script_hash(
    config: &RollupConfig,
    index: usize,
    source: Source,
) -> Result<Option<[u8; 32]>, Error> {
    match load_cell_type(index, source)? {
        Some(type_) => {
            if type_.hash_type() == ScriptHashType::Type.into()
                && type_.code_hash().as_slice() == config.l1_sudt_type_hash().as_slice()
            {
                return Ok(load_cell_type_hash(index, source)?);
            }
            Err(Error::SUDT)
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
