use anyhow::Result;
use gw_types::h256::H256;
use sha2::{Digest, Sha256};
use std::path::Path;

pub fn content_checksum(content: &[u8]) -> Result<H256> {
    let mut hasher = Sha256::new();
    hasher.update(&content);
    Ok(hasher.finalize().into())
}

pub fn file_checksum<P: AsRef<Path>>(path: P) -> Result<H256> {
    let content = std::fs::read(path)?;
    content_checksum(&content)
}
