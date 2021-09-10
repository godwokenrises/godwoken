use anyhow::{bail, Result};

pub fn to_h256(input: &str) -> Result<[u8; 32]> {
    let input = hex::decode(input.strip_prefix("0x").unwrap())?;
    if input.len() != 32 {
        bail!("invalid input len: {}", input.len());
    }
    let mut buf = [0u8; 32];
    buf.copy_from_slice(&input);
    Ok(buf)
}
