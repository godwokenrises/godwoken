use std::convert::TryInto;

use anyhow::{anyhow, Result};

pub fn to_h256(input: &str) -> Result<[u8; 32]> {
    let input = hex::decode(input.trim_start_matches("0x"))?;
    input[..]
        .try_into()
        .map_err(|_| anyhow!("invalid input len: {}", input.len()))
}
