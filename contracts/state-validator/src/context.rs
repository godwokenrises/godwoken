//! state context
//! supports read / write to global state

use crate::blake2b::new_blake2b;
use crate::error::Error;
use crate::key::{build_account_key, build_raw_key, GW_ACCOUNT_NONCE, GW_ACCOUNT_PUBKEY_HASH};
use alloc::collections::BTreeMap;
use sparse_merkle_tree::H256;

pub struct Context {
    pub number: u64,
    pub aggregator_id: u32,
    pub kv_pairs: BTreeMap<H256, H256>,
    pub account_count: u32,
    pub rollup_type_id: [u8; 32],
}

impl Context {
    pub fn create_account(&mut self, pubkey_hash: [u8; 20]) -> Result<u32, Error> {
        let id = self.account_count;
        // Account is composited by (pubkey_hash, code_hash, nonce, kv)
        // for a new account, the nonce is 0 and kv is empty
        // since we do not allow create an account with code_hash via this interface,
        // the code_hash is also empty, we can skip these fields.
        // the only field we need to insert is pubkey_hash
        {
            let pubkey_hash_key = build_account_key(id, GW_ACCOUNT_PUBKEY_HASH);
            let mut pubkey_hash_value = [0u8; 32];
            pubkey_hash_value[..pubkey_hash.len()].copy_from_slice(&pubkey_hash);
            self.kv_pairs
                .insert(pubkey_hash_key.into(), pubkey_hash_value.into());
        }
        self.account_count += 1;
        Ok(id)
    }

    pub fn mint_sudt(&mut self, token_id: &[u8; 32], id: u32, amount: u128) -> Result<(), Error> {
        // build application key
        let sudt_key = {
            let mut buf = [0u8; 32];
            let mut hasher = new_blake2b();
            hasher.update(token_id);
            hasher.update(&id.to_le_bytes());
            hasher.finalize(&mut buf);
            buf
        };
        // build low-level key
        let key = build_raw_key(id, &sudt_key);

        // calculate balance
        let mut balance = self
            .kv_pairs
            .get(&key.into())
            .map(|value| {
                let mut buf = [0u8; 16];
                buf.copy_from_slice(&value.as_slice()[..16]);
                u128::from_le_bytes(buf)
            })
            .unwrap_or(0);
        balance = balance.checked_add(amount).ok_or(Error::AmountOverflow)?;
        let mut value = [0u8; 32];
        value[..16].copy_from_slice(&balance.to_le_bytes());
        self.kv_pairs.insert(key.into(), value.into());
        Ok(())
    }

    pub fn get_pubkey_hash(&self, id: u32) -> Option<[u8; 20]> {
        let raw_key = build_account_key(id, GW_ACCOUNT_PUBKEY_HASH);
        self.kv_pairs.get(&raw_key.into()).map(|value| {
            let mut pubkey_hash = [0u8; 20];
            pubkey_hash.copy_from_slice(value.as_slice());
            pubkey_hash
        })
    }

    pub fn get_nonce(&self, id: u32) -> Option<u32> {
        let raw_key = build_account_key(id, GW_ACCOUNT_NONCE);
        self.kv_pairs.get(&raw_key.into()).map(|value| {
            let mut buf = [0u8; 4];
            buf.copy_from_slice(value.as_slice());
            u32::from_le_bytes(buf)
        })
    }
}
