//! One physical SMT, bounded account pages, and typed global records.
use super::*;
pub const STATE: usize = 0;
pub const PAGE_ITEMS: usize = 16;
pub const MAX_PAGE_BYTES: usize = 8192;
pub type Page = BTreeMap<(usize, Hash), Vec<u8>>;
// These are physical SMT keys; logical record keys remain inside account pages.
pub fn account_key(id: u32, page: u32) -> Hash {
    let mut k = [0; 32];
    k[..4].copy_from_slice(&id.to_le_bytes());
    k[4..8].copy_from_slice(&page.to_le_bytes());
    k[31] = 16;
    k
}
pub fn asset_state_key(id: u32) -> Hash {
    let mut k = [0; 32];
    k[..4].copy_from_slice(&id.to_le_bytes());
    k[31] = 32;
    k
}
pub fn epoch_state_key(id: u64) -> Hash {
    let mut k = [0; 32];
    k[..8].copy_from_slice(&id.to_le_bytes());
    k[31] = 48;
    k
}
pub fn namespace(k: Hash) -> Result<u8> {
    // Reserved bytes must be zero so each account page, asset and epoch has one encoding.
    match k[31] {
        16 => {
            require(k[8..31].iter().all(|b| *b == 0), Error::Keys)?;
            Ok(16)
        }
        32 => {
            require(k[4..31].iter().all(|b| *b == 0), Error::Keys)?;
            Ok(32)
        }
        48 => {
            require(k[8..31].iter().all(|b| *b == 0), Error::Keys)?;
            Ok(48)
        }
        _ => Err(Error::Keys),
    }
}
pub fn static_kind(kind: usize) -> Result<usize> {
    match kind {
        PAID => Ok(CLAIM_PLAN),
        RESERVES => Ok(LOT_PLAN),
        SOURCES => Ok(SOURCE_PLAN),
        SOURCE_PLAN | LOT_PLAN | CLAIM_PLAN => Ok(kind),
        _ => Err(Error::Keys),
    }
}
pub fn mutable(kind: usize) -> bool {
    matches!(kind, PAID | RESERVES | SOURCES | ASSETS)
}
pub fn pack_page(map: &Page) -> Vec<u8> {
    let mut out = Vec::new();
    601u32.write(&mut out);
    (map.len() as u32).write(&mut out);
    for ((kind, key), value) in map {
        (*kind as u32).write(&mut out);
        key.write(&mut out);
        (value.len() as u32).write(&mut out);
        out.extend_from_slice(value);
    }
    out
}
pub fn unpack_page(raw: &[u8]) -> Result<Page> {
    require(!raw.is_empty() && raw.len() <= MAX_PAGE_BYTES, Error::Limit)?;
    let mut c = Cursor(raw);
    require(u32::read(&mut c)? == 601, Error::Encoding)?;
    let n = u32::read(&mut c)? as usize;
    require(n > 0 && n <= PAGE_ITEMS * 2, Error::Limit)?;
    let mut map = Page::new();
    let mut immutable = 0;
    for _ in 0..n {
        let kind = u32::read(&mut c)? as usize;
        let key = Hash::read(&mut c)?;
        let len = u32::read(&mut c)? as usize;
        let sk = static_kind(kind)?;
        let (tag, size) = match kind {
            SOURCE_PLAN => (512, Source::SIZE),
            LOT_PLAN => (513, Lot::SIZE),
            CLAIM_PLAN => (515, Claim::SIZE),
            PAID => (520, Paid::SIZE),
            RESERVES => (514, Reserve::SIZE),
            SOURCES => (516, SourceBudget::SIZE),
            _ => return Err(Error::Keys),
        };
        require(
            len == size && c.0.len() >= len && c.0[..4] == (tag as u32).to_le_bytes(),
            Error::Encoding,
        )?;
        if sk == kind {
            immutable += 1;
        }
        require(
            map.insert((kind, key), c.0[..len].to_vec()).is_none(),
            Error::Keys,
        )?;
        c.0 = &c.0[len..];
    }
    // Repacking rejects noncanonical order as well as trailing bytes.
    require(
        immutable > 0 && immutable <= PAGE_ITEMS && c.0.is_empty() && pack_page(&map) == raw,
        Error::Encoding,
    )?;
    for ((kind, key), value) in &map {
        if mutable(*kind) {
            require(
                map.contains_key(&(static_kind(*kind)?, *key)) && value.len() == 36,
                Error::Keys,
            )?;
        }
    }
    Ok(map)
}
pub fn record_index(proofs: &[TreeProof], kind: usize) -> Result<BTreeMap<Hash, Vec<u8>>> {
    require(proofs.len() == 1 && proofs[0].kind == STATE, Error::Proof)?;
    let mut found = BTreeMap::new();
    for (k, raw) in &proofs[0].leaves {
        if namespace(*k)? == 16 {
            let page = unpack_page(raw)?;
            for ((tag, key), value) in page {
                if tag == kind {
                    require(found.insert(key, value).is_none(), Error::Keys)?;
                }
            }
        }
    }
    Ok(found)
}
