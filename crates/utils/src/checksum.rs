use anyhow::Result;
use gw_types::h256::H256;
use sha2::{Digest, Sha256};
use std::path::Path;

pub fn content_checksum(content: &[u8]) -> H256 {
    Sha256::digest(content).into()
}

pub fn file_checksum<P: AsRef<Path>>(path: P) -> Result<H256> {
    let content = std::fs::read(path)?;
    Ok(content_checksum(&content))
}
