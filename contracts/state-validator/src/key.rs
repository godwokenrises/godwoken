//! utilities to generate low-level key

use crate::blake2b::new_blake2b;
use core::mem::size_of;

/* key type */
const GW_ACCOUNT_KV: u8 = 0;
// pub const GW_ACCOUNT_NONCE: u8 = 1;
pub const GW_ACCOUNT_PUBKEY_HASH: u8 = 2;
// pub const GW_ACCOUNT_CODE_HASH: u8 = 3;

/* Generate raw key
 * raw_key: blake2b(id | type | key)
 *
 * We use raw key in the underlying KV store
 */
pub fn build_raw_key(id: u32, key: &[u8]) -> [u8; 32] {
    let mut raw_key = [0u8; 32];
    let mut hasher = new_blake2b();
    hasher.update(&id.to_le_bytes());
    hasher.update(&[GW_ACCOUNT_KV]);
    hasher.update(key);
    hasher.finalize(&mut raw_key);
    raw_key
}

pub fn build_account_key(id: u32, type_: u8) -> [u8; 32] {
    let mut key = [0u8; 32];
    key[..size_of::<u32>()].copy_from_slice(&id.to_le_bytes());
    key[size_of::<u32>()] = type_;
    key
}
