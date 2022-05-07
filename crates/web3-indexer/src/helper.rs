use anyhow::Result;
use gw_common::H256;
use std::{convert::TryInto, usize};

#[derive(Default, Debug)]
pub struct PolyjuiceArgs {
    pub is_create: bool,
    pub gas_limit: u64,
    pub gas_price: u128,
    pub value: u128,
    pub input: Option<Vec<u8>>,
}

impl PolyjuiceArgs {
    // https://github.com/nervosnetwork/godwoken-polyjuice/blob/v0.6.0-rc1/polyjuice-tests/src/helper.rs#L322
    pub fn decode(args: &[u8]) -> anyhow::Result<Self> {
        let is_create = args[7] == 3u8;
        let gas_limit = u64::from_le_bytes(args[8..16].try_into()?);
        let gas_price = u128::from_le_bytes(args[16..32].try_into()?);
        let value = u128::from_le_bytes(args[32..48].try_into()?);
        let input_size = u32::from_le_bytes(args[48..52].try_into()?);
        let input: Vec<u8> = args[52..(52 + input_size as usize)].to_vec();
        Ok(PolyjuiceArgs {
            is_create,
            gas_limit,
            gas_price,
            value,
            input: Some(input),
        })
    }
}

pub fn account_script_hash_to_eth_address(account_script_hash: H256) -> [u8; 20] {
    let mut data = [0u8; 20];
    data.copy_from_slice(&account_script_hash.as_slice()[0..20]);
    data
}

pub fn hex(raw: &[u8]) -> Result<String> {
    Ok(format!("0x{}", faster_hex::hex_string(raw)?))
}
