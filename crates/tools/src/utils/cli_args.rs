use std::str::FromStr;

use anyhow::{anyhow, Result};
use ckb_fixed_hash::{H160, H256};

// Like H256/H160 but FromStr allows 0x prefixed values too.

#[derive(Default)]
pub struct H256Arg(pub H256);

#[derive(Default)]
pub struct H160Arg(pub H160);

impl FromStr for H160Arg {
    type Err = anyhow::Error;
    fn from_str(value: &str) -> Result<Self> {
        let h = value.strip_prefix("0x").unwrap_or(value).parse()?;
        Ok(Self(h))
    }
}

impl FromStr for H256Arg {
    type Err = anyhow::Error;
    fn from_str(value: &str) -> Result<Self> {
        let h = value.strip_prefix("0x").unwrap_or(value).parse()?;
        Ok(Self(h))
    }
}

pub fn to_h256(input: &str) -> Result<[u8; 32]> {
    let input = hex::decode(input.trim_start_matches("0x"))?;
    input[..]
        .try_into()
        .map_err(|_| anyhow!("invalid input len: {}", input.len()))
}
