use anyhow::{bail, Result};

pub fn to_h256(input: &str) -> Result<[u8; 32]> {
    let input = hex::decode(input.trim_start_matches("0x"))?;
    if input.len() != 32 {
        bail!("invalid input len: {}", input.len());
    }
    let mut buf = [0u8; 32];
    buf.copy_from_slice(&input);
    Ok(buf)
}
