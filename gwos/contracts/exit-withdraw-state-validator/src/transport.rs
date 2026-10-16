//! Bounded witness framing. Extra witnesses are also authenticated by standard secp sighash-all.
use crate::{
    verify::{require, MAX_ACTION},
    Error, Result,
};
use alloc::vec::Vec;
pub const CHUNK: usize = 16000;
pub const MAGIC: &[u8; 8] = b"GWCHNK01";
pub fn frame(raw: &[u8]) -> (Vec<u8>, Vec<Vec<u8>>) {
    if raw.len() <= CHUNK {
        return (raw.to_vec(), Vec::new());
    }
    let parts: Vec<Vec<u8>> = raw.chunks(CHUNK).map(|v| v.to_vec()).collect();
    let mut header = MAGIC.to_vec();
    header.extend_from_slice(&(raw.len() as u32).to_le_bytes());
    header.extend_from_slice(&(parts.len() as u32).to_le_bytes());
    header.extend_from_slice(&gw_hash::blake2b::hash(raw));
    (header, parts)
}
pub fn unframe(
    header: &[u8],
    start: usize,
    mut load: impl FnMut(usize) -> Result<Vec<u8>>,
) -> Result<Vec<u8>> {
    if !header.starts_with(MAGIC) {
        require(header.len() <= MAX_ACTION, Error::Limit)?;
        return Ok(header.to_vec());
    }
    require(header.len() == 48, Error::Encoding)?;
    let n = u32::from_le_bytes(header[8..12].try_into().map_err(|_| Error::Encoding)?) as usize;
    let count =
        u32::from_le_bytes(header[12..16].try_into().map_err(|_| Error::Encoding)?) as usize;
    require(
        n > CHUNK && n <= MAX_ACTION && count == (n + CHUNK - 1) / CHUNK,
        Error::Limit,
    )?;
    let mut raw = Vec::with_capacity(n);
    for i in 0..count {
        let part = load(start.checked_add(i).ok_or(Error::Overflow)?)?;
        require(part.len() == CHUNK.min(n - i * CHUNK), Error::Encoding)?;
        raw.extend_from_slice(&part);
    }
    require(
        gw_hash::blake2b::hash(&raw).as_slice() == &header[16..],
        Error::Proof,
    )?;
    Ok(raw)
}
