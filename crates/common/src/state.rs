//! State
// the State keeps consistent interface upon lower storage structure (SMT or other trees).
//
// Key domain:
//
// The account and some special values used in the Godwoken are persisted in the state kv storage.
// We serperate these keys into different key domains.
//
// Any `key` in the State must be converted into `raw_key`:
//
// raw_key: blake2b(id(4 bytes) | type(1 byte) | key(32 bytes))
//
// - `id` represents account's id, id must be set to 0 if the key isn't belong to an account
// - `type` is a domain separator, different type of keys must use a different `type`
// - `key` the original key
//
// Thus, the first 5 bytes keeps uniqueness for different type of keys.

use crate::error::Error;
use crate::h256_ext::{H256Ext, H256};
use crate::vec::Vec;
use crate::{blake2b::new_blake2b, merkle_utils::calculate_state_checkpoint};
use core::mem::size_of;

/* Account fields types */
pub const GW_ACCOUNT_KV_TYPE: u8 = 0;
pub const GW_ACCOUNT_NONCE_TYPE: u8 = 1;
pub const GW_ACCOUNT_SCRIPT_HASH_TYPE: u8 = 2;
/* Non-account types */
pub const GW_NON_ACCOUNT_PLACEHOLDER: [u8; 4] = [0u8; 4];
pub const GW_SCRIPT_HASH_TO_ID_TYPE: u8 = 3;
pub const GW_DATA_HASH_TYPE: u8 = 4;
pub const GW_SHORT_SCRIPT_HASH_TO_SCRIPT_HASH_TYPE: u8 = 5;

pub const SUDT_KEY_FLAG_BALANCE: u32 = 1;

// 20 Bytes
pub const DEFAULT_SHORT_SCRIPT_HASH_LEN: usize = 20;

/* Generate a SMT key
 * raw_key: blake2b(id | type | key)
 *
 * We use raw key in the underlying KV store
 */
pub fn build_account_key(id: u32, key: &[u8]) -> H256 {
    let mut raw_key = [0u8; 32];
    let mut hasher = new_blake2b();
    hasher.update(&id.to_le_bytes());
    hasher.update(&[GW_ACCOUNT_KV_TYPE]);
    hasher.update(key);
    hasher.finalize(&mut raw_key);
    raw_key.into()
}

pub fn build_sudt_key(key_flag: u32, short_address: &[u8]) -> Vec<u8> {
    let mut key = Vec::with_capacity(short_address.len() + 8);
    key.extend(&key_flag.to_le_bytes());
    key.extend(&(short_address.len() as u32).to_le_bytes());
    key.extend(short_address);
    key
}

pub fn build_account_field_key(id: u32, type_: u8) -> H256 {
    let mut key: [u8; 32] = H256::zero().into();
    key[..size_of::<u32>()].copy_from_slice(&id.to_le_bytes());
    key[size_of::<u32>()] = type_;
    key.into()
}

pub fn build_script_hash_to_account_id_key(script_hash: &[u8]) -> H256 {
    let mut key: [u8; 32] = H256::zero().into();
    let mut hasher = new_blake2b();
    hasher.update(&GW_NON_ACCOUNT_PLACEHOLDER);
    hasher.update(&[GW_SCRIPT_HASH_TO_ID_TYPE]);
    hasher.update(script_hash);
    hasher.finalize(&mut key);
    key.into()
}

pub fn build_data_hash_key(data_hash: &[u8]) -> H256 {
    let mut key: [u8; 32] = H256::zero().into();
    let mut hasher = new_blake2b();
    hasher.update(&GW_NON_ACCOUNT_PLACEHOLDER);
    hasher.update(&[GW_DATA_HASH_TYPE]);
    hasher.update(data_hash);
    hasher.finalize(&mut key);
    key.into()
}

pub fn build_short_script_hash_to_script_hash_key(short_script_hash: &[u8]) -> H256 {
    let mut key: [u8; 32] = H256::zero().into();
    let mut hasher = new_blake2b();
    hasher.update(&GW_NON_ACCOUNT_PLACEHOLDER);
    hasher.update(&[GW_SHORT_SCRIPT_HASH_TO_SCRIPT_HASH_TYPE]);
    let len = short_script_hash.len() as u32;
    hasher.update(&len.to_le_bytes());
    hasher.update(short_script_hash);
    hasher.finalize(&mut key);
    key.into()
}

/// NOTE: the length `20` is a hard-coded value, may be `16` for some LockAlgorithm.
pub fn to_short_address(script_hash: &H256) -> &[u8] {
    &script_hash.as_slice()[0..20]
}

pub struct PrepareWithdrawalRecord {
    pub withdrawal_lock_hash: H256,
    pub amount: u128,
    pub block_number: u64,
}

pub trait State {
    // KV interface
    fn get_raw(&self, key: &H256) -> Result<H256, Error>;
    fn update_raw(&mut self, key: H256, value: H256) -> Result<(), Error>;
    fn get_account_count(&self) -> Result<u32, Error>;
    fn set_account_count(&mut self, count: u32) -> Result<(), Error>;
    fn calculate_root(&self) -> Result<H256, Error>;

    // implementations
    fn get_value(&self, id: u32, key: &H256) -> Result<H256, Error> {
        let raw_key = build_account_key(id, key.as_slice());
        self.get_raw(&raw_key)
    }
    fn update_value(&mut self, id: u32, key: &H256, value: H256) -> Result<(), Error> {
        let raw_key = build_account_key(id, key.as_slice());
        self.update_raw(raw_key, value)?;
        Ok(())
    }
    /// Create a new account
    fn create_account(&mut self, script_hash: H256) -> Result<u32, Error> {
        let id = self.get_account_count()?;
        // nonce
        self.set_nonce(id, 0)?;
        // script hash
        self.update_raw(
            build_account_field_key(id, GW_ACCOUNT_SCRIPT_HASH_TYPE),
            script_hash,
        )?;
        // script hash to id
        self.update_raw(
            build_script_hash_to_account_id_key(script_hash.as_slice()),
            H256::from_u32(id),
        )?;
        // short script hash to script hash
        self.update_raw(
            build_short_script_hash_to_script_hash_key(
                &script_hash.as_slice()[..DEFAULT_SHORT_SCRIPT_HASH_LEN],
            ),
            script_hash,
        )?;
        // update account count
        self.set_account_count(id + 1)?;
        Ok(id)
    }

    fn get_script_hash(&self, id: u32) -> Result<H256, Error> {
        let value = self.get_raw(&build_account_field_key(id, GW_ACCOUNT_SCRIPT_HASH_TYPE))?;
        Ok(value)
    }

    fn get_nonce(&self, id: u32) -> Result<u32, Error> {
        let value = self.get_raw(&build_account_field_key(id, GW_ACCOUNT_NONCE_TYPE))?;
        Ok(value.to_u32())
    }

    fn set_nonce(&mut self, id: u32, nonce: u32) -> Result<(), Error> {
        self.update_raw(
            build_account_field_key(id, GW_ACCOUNT_NONCE_TYPE),
            H256::from_u32(nonce),
        )?;
        Ok(())
    }

    fn get_account_id_by_script_hash(&self, script_hash: &H256) -> Result<Option<u32>, Error> {
        let value = self.get_raw(&build_script_hash_to_account_id_key(script_hash.as_slice()))?;
        if value.is_zero() {
            return Ok(None);
        }
        let id = value.to_u32();
        Ok(Some(id))
    }

    fn get_sudt_balance(&self, sudt_id: u32, short_address: &[u8]) -> Result<u128, Error> {
        if short_address.len() != 20 {
            return Err(Error::InvalidShortAddress);
        }
        // get balance
        let sudt_key = build_sudt_key(SUDT_KEY_FLAG_BALANCE, short_address);
        let balance = self.get_raw(&build_account_key(sudt_id, &sudt_key))?;
        Ok(balance.to_u128())
    }

    fn store_data_hash(&mut self, data_hash: H256) -> Result<(), Error> {
        let key = build_data_hash_key(data_hash.as_slice());
        self.update_raw(key, H256::one())?;
        Ok(())
    }

    fn is_data_hash_exist(&self, data_hash: &H256) -> Result<bool, Error> {
        let key = build_data_hash_key(data_hash.as_slice());
        let v = self.get_raw(&key)?;
        Ok(v == H256::one())
    }

    /// Mint SUDT token on layer2
    fn mint_sudt(&mut self, sudt_id: u32, short_address: &[u8], amount: u128) -> Result<(), Error> {
        if short_address.len() != 20 {
            return Err(Error::InvalidShortAddress);
        }
        let sudt_key = build_sudt_key(SUDT_KEY_FLAG_BALANCE, short_address);
        let raw_key = build_account_key(sudt_id, &sudt_key);
        // calculate balance
        let mut balance = self.get_raw(&raw_key)?.to_u128();
        balance = balance.checked_add(amount).ok_or(Error::AmountOverflow)?;
        self.update_raw(raw_key, H256::from_u128(balance))?;
        Ok(())
    }

    /// burn SUDT
    fn burn_sudt(&mut self, sudt_id: u32, short_address: &[u8], amount: u128) -> Result<(), Error> {
        if short_address.len() != 20 {
            return Err(Error::InvalidShortAddress);
        }
        let sudt_key = build_sudt_key(SUDT_KEY_FLAG_BALANCE, short_address);
        let raw_key = build_account_key(sudt_id, &sudt_key);
        // calculate balance
        let mut balance = self.get_raw(&raw_key)?.to_u128();
        balance = balance.checked_sub(amount).ok_or(Error::AmountOverflow)?;
        self.update_raw(raw_key, H256::from_u128(balance))?;
        Ok(())
    }

    /// calculate state checkpoint
    fn calculate_state_checkpoint(&self) -> Result<H256, Error> {
        let account_root = self.calculate_root()?;
        let account_count = self.get_account_count()?;
        Ok(calculate_state_checkpoint(&account_root, account_count))
    }
}
