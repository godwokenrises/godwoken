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

use gw_types::{core::H256, U256};

use crate::builtins::ETH_REGISTRY_ACCOUNT_ID;
use crate::error::Error;
use crate::registry_address::RegistryAddress;
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

/* Simple UDT key flag */
pub const SUDT_KEY_FLAG_BALANCE: u32 = 1;
pub const SUDT_TOTAL_SUPPLY_KEY: [u8; 32] = [0xff; 32];

/* Registry key flag */
pub const REGISTRY_KEY_PREFIX: &[u8; 3] = b"reg";
pub const REGISTRY_KEY_FLAG_SCRIPT_HASH_TO_NATIVE: u8 = 1;
pub const REGISTRY_KEY_FLAG_NATIVE_TO_SCRIPT_HASH: u8 = 2;

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

pub fn build_sudt_key(key_flag: u32, address: &RegistryAddress) -> Vec<u8> {
    let mut key = Vec::default();
    key.resize(address.len() + 4, 0);
    key[..4].copy_from_slice(&key_flag.to_le_bytes());
    address.write_to_slice(&mut key[4..]).unwrap();
    key
}

/// format: "reg" | flag(1 bytes) | script_hash(32 bytes)
pub fn build_script_hash_to_registry_address_key(script_hash: &H256) -> Vec<u8> {
    let mut key = Vec::default();
    key.resize(36, 0);
    key[..3].copy_from_slice(REGISTRY_KEY_PREFIX);
    key[3] = REGISTRY_KEY_FLAG_SCRIPT_HASH_TO_NATIVE;
    key[4..].copy_from_slice(script_hash.as_slice());
    key
}

/// format: "reg" | flag(1 byte) | registry_address
/// registry_address: registry_id(4 bytes) | address_len(4 bytes) | address(n bytes)
pub fn build_registry_address_to_script_hash_key(address: &RegistryAddress) -> Vec<u8> {
    let mut key = Vec::default();
    key.resize(4 + address.len(), 0);
    key[..3].copy_from_slice(REGISTRY_KEY_PREFIX);
    key[3] = REGISTRY_KEY_FLAG_NATIVE_TO_SCRIPT_HASH;
    address.write_to_slice(&mut key[4..]).unwrap();
    key
}

pub fn build_account_field_key(id: u32, type_: u8) -> H256 {
    let mut key: [u8; 32] = H256::zero().into();
    key[..size_of::<u32>()].copy_from_slice(&id.to_le_bytes());
    key[size_of::<u32>()] = type_;
    key.into()
}

/// build_script_hash_to_account_id_key
/// value format:
/// id(4 bytes) | exists flag(1 byte) | zeros bytes
/// if script_hash is exists the exists flag turn into 1, otherwise it is 0.
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
    fn get_value(&self, id: u32, key: &[u8]) -> Result<H256, Error> {
        assert!(!key.is_empty());
        let raw_key = build_account_key(id, key);
        self.get_raw(&raw_key)
    }
    fn update_value(&mut self, id: u32, key: &[u8], value: H256) -> Result<(), Error> {
        assert!(!key.is_empty());
        let raw_key = build_account_key(id, key);
        self.update_raw(raw_key, value)?;
        Ok(())
    }
    /// Create a new account
    fn create_account(&mut self, script_hash: H256) -> Result<u32, Error> {
        // check duplication
        if self.get_account_id_by_script_hash(&script_hash)?.is_some() {
            return Err(Error::DuplicatedScriptHash);
        }
        let id = self.get_account_count()?;
        // nonce
        self.set_nonce(id, 0)?;
        // script hash
        self.update_raw(
            build_account_field_key(id, GW_ACCOUNT_SCRIPT_HASH_TYPE),
            script_hash,
        )?;
        // script hash to id
        let script_hash_to_id_value: H256 = {
            let mut buf: [u8; 32] = H256::from_u32(id).into();
            // the first 4 bytes is id, set exists flag(fifth byte) to 1
            buf[4] = 1;
            buf.into()
        };
        self.update_raw(
            build_script_hash_to_account_id_key(script_hash.as_slice()),
            script_hash_to_id_value,
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

    fn get_sudt_balance(&self, sudt_id: u32, address: &RegistryAddress) -> Result<U256, Error> {
        // get balance
        let sudt_key = build_sudt_key(SUDT_KEY_FLAG_BALANCE, address);
        let balance = self.get_value(sudt_id, &sudt_key)?;
        Ok(balance.to_u256())
    }

    fn get_sudt_total_supply(&self, sudt_id: u32) -> Result<U256, Error> {
        let total_supply = self.get_value(sudt_id, &SUDT_TOTAL_SUPPLY_KEY)?;
        Ok(total_supply.to_u256())
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
    fn mint_sudt(
        &mut self,
        sudt_id: u32,
        address: &RegistryAddress,
        amount: U256,
    ) -> Result<(), Error> {
        let sudt_key = build_sudt_key(SUDT_KEY_FLAG_BALANCE, address);
        let raw_key = build_account_key(sudt_id, &sudt_key);
        // calculate balance
        let mut balance = self.get_raw(&raw_key)?.to_u256();
        balance = balance.checked_add(amount).ok_or(Error::AmountOverflow)?;
        self.update_raw(raw_key, H256::from_u256(balance))?;

        // update total supply
        let raw_key = build_account_key(sudt_id, &SUDT_TOTAL_SUPPLY_KEY);
        let mut total_supply = self.get_raw(&raw_key)?.to_u256();
        total_supply = total_supply
            .checked_add(amount)
            .ok_or(Error::AmountOverflow)?;
        self.update_raw(raw_key, H256::from_u256(total_supply))?;

        Ok(())
    }

    /// burn SUDT
    fn burn_sudt(
        &mut self,
        sudt_id: u32,
        address: &RegistryAddress,
        amount: U256,
    ) -> Result<(), Error> {
        let sudt_key = build_sudt_key(SUDT_KEY_FLAG_BALANCE, address);
        let raw_key = build_account_key(sudt_id, &sudt_key);
        // calculate balance
        let mut balance = self.get_raw(&raw_key)?.to_u256();
        balance = balance.checked_sub(amount).ok_or(Error::AmountOverflow)?;
        self.update_raw(raw_key, H256::from_u256(balance))?;

        // update total supply
        let raw_key = build_account_key(sudt_id, &SUDT_TOTAL_SUPPLY_KEY);
        let mut total_supply = self.get_raw(&raw_key)?.to_u256();
        total_supply = total_supply
            .checked_sub(amount)
            .ok_or(Error::AmountOverflow)?;
        self.update_raw(raw_key, H256::from_u256(total_supply))?;

        Ok(())
    }

    /// calculate state checkpoint
    fn calculate_state_checkpoint(&self) -> Result<H256, Error> {
        let account_root = self.calculate_root()?;
        let account_count = self.get_account_count()?;
        Ok(calculate_state_checkpoint(&account_root, account_count))
    }

    fn get_script_hash_by_registry_address(
        &self,
        address: &RegistryAddress,
    ) -> Result<Option<H256>, Error> {
        let key = build_registry_address_to_script_hash_key(address);
        let value = self.get_value(address.registry_id, &key)?;
        if value.is_zero() {
            return Ok(None);
        }
        Ok(Some(value))
    }

    fn get_registry_address_by_script_hash(
        &self,
        registry_id: u32,
        script_hash: &H256,
    ) -> Result<Option<RegistryAddress>, Error> {
        let key = build_script_hash_to_registry_address_key(script_hash);
        let value = self.get_value(registry_id, &key)?;
        if value.is_zero() {
            return Ok(None);
        }
        Ok(Some(RegistryAddress::from_slice(value.as_slice()).unwrap()))
    }

    /// This function create a bi-direction mapping between registry address & script_hash
    fn mapping_registry_address_to_script_hash(
        &mut self,
        addr: RegistryAddress,
        script_hash: H256,
    ) -> Result<(), Error> {
        // Only support addr len == 20 for now, we can revisit the condition in later version
        if addr.address.len() != 20 {
            return Err(Error::InvalidArgs);
        }
        if script_hash.is_zero() {
            return Err(Error::InvalidArgs);
        }
        if addr.registry_id != ETH_REGISTRY_ACCOUNT_ID {
            return Err(Error::InvalidArgs);
        }
        // Check duplication
        if self
            .get_registry_address_by_script_hash(addr.registry_id, &script_hash)?
            .is_some()
        {
            return Err(Error::DuplicatedRegistryAddress);
        }
        if self.get_script_hash_by_registry_address(&addr)?.is_some() {
            return Err(Error::DuplicatedRegistryAddress);
        }
        // script hash -> registry address
        {
            let key = build_script_hash_to_registry_address_key(&script_hash);
            let mut addr_buf = [0u8; 32];
            addr.write_to_slice(&mut addr_buf)
                .expect("write addr to buf");
            self.update_value(addr.registry_id, &key, addr_buf.into())?;
        }
        // registry address -> script hash
        {
            let key = build_registry_address_to_script_hash_key(&addr);
            self.update_value(addr.registry_id, &key, script_hash)?;
        }
        Ok(())
    }
}
