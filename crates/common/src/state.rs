use crate::blake2b::new_blake2b;
use crate::error::Error;
use crate::h256_ext::{H256Ext, H256};
use core::mem::size_of;

/* Account fields flags */
pub const GW_ACCOUNT_KV: u8 = 0;
pub const GW_ACCOUNT_NONCE: u8 = 1;
pub const GW_ACCOUNT_SCRIPT_HASH: u8 = 2;
pub const GW_ACCOUNT_SCRIPT_HASH_TO_ID: u8 = 3;

/* SUDT fields */
const WITHDRAWAL_LOCK_HASH: u8 = 1;
const WITHDRAWAL_AMOUNT: u8 = 2;
const WITHDRAWAL_BLOCK_NUMBER: u8 = 3;

/* Generate a SMT key
 * raw_key: blake2b(id | type | key)
 *
 * We use raw key in the underlying KV store
 */
pub fn build_account_key(id: u32, key: &[u8]) -> H256 {
    let mut raw_key = [0u8; 32];
    let mut hasher = new_blake2b();
    hasher.update(&id.to_le_bytes());
    hasher.update(&[GW_ACCOUNT_KV]);
    hasher.update(key);
    hasher.finalize(&mut raw_key);
    raw_key.into()
}

pub fn build_account_field_key(id: u32, type_: u8) -> H256 {
    let mut key: [u8; 32] = H256::zero().into();
    key[..size_of::<u32>()].copy_from_slice(&id.to_le_bytes());
    key[size_of::<u32>()] = type_;
    key.into()
}

fn build_sudt_field_key(id: u32, type_: u8) -> H256 {
    let mut key: [u8; 32] = H256::zero().into();
    key[..size_of::<u32>()].copy_from_slice(&id.to_le_bytes());
    key[size_of::<u32>()] = type_;
    key.into()
}

pub fn build_script_hash_to_account_id_key(script_hash: &[u8]) -> H256 {
    let mut key: [u8; 32] = H256::zero().into();
    let mut hasher = new_blake2b();
    hasher.update(&[GW_ACCOUNT_SCRIPT_HASH_TO_ID]);
    hasher.update(script_hash);
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
        self.update_raw(
            build_account_field_key(id, GW_ACCOUNT_NONCE).into(),
            H256::zero(),
        )?;
        // script hash
        self.update_raw(
            build_account_field_key(id, GW_ACCOUNT_SCRIPT_HASH).into(),
            script_hash.into(),
        )?;
        // script hash to id
        self.update_raw(
            build_script_hash_to_account_id_key(&script_hash.as_slice()).into(),
            H256::from_u32(id),
        )?;
        // update account count
        self.set_account_count(id + 1)?;
        Ok(id)
    }
    fn get_script_hash(&self, id: u32) -> Result<H256, Error> {
        let value = self.get_raw(&build_account_field_key(id, GW_ACCOUNT_SCRIPT_HASH).into())?;
        Ok(value.into())
    }
    fn get_nonce(&self, id: u32) -> Result<u32, Error> {
        let value = self.get_raw(&build_account_field_key(id, GW_ACCOUNT_NONCE).into())?;
        Ok(value.to_u32())
    }

    fn get_account_id_by_script_hash(&self, script_hash: &H256) -> Result<Option<u32>, Error> {
        let value =
            self.get_raw(&build_script_hash_to_account_id_key(script_hash.as_slice()).into())?;
        if value.is_zero() {
            return Ok(None);
        }
        let id = value.to_u32();
        Ok(Some(id))
    }

    fn get_sudt_balance(&self, sudt_id: u32, id: u32) -> Result<u128, Error> {
        // get balance
        let balance = self.get_value(sudt_id, &H256::from_u32(id))?;
        Ok(balance.to_u128())
    }

    /// Mint SUDT token on layer2
    fn mint_sudt(&mut self, sudt_id: u32, id: u32, amount: u128) -> Result<(), Error> {
        let raw_key = build_account_key(sudt_id, &H256::from_u32(id).as_slice());
        // calculate balance
        let mut balance = self.get_raw(&raw_key)?.to_u128();
        balance = balance.checked_add(amount).ok_or(Error::AmountOverflow)?;
        self.update_raw(raw_key, H256::from_u128(balance))?;
        Ok(())
    }

    /// Get SUDT prepare withdrawal record
    fn get_prepare_withdrawal(
        &mut self,
        sudt_id: u32,
        id: u32,
    ) -> Result<PrepareWithdrawalRecord, Error> {
        let withdrawal_lock_hash =
            self.get_value(sudt_id, &build_sudt_field_key(id, WITHDRAWAL_LOCK_HASH))?;
        let block_number = self
            .get_value(sudt_id, &build_sudt_field_key(id, WITHDRAWAL_BLOCK_NUMBER))?
            .to_u64();
        let amount = self
            .get_value(sudt_id, &build_sudt_field_key(id, WITHDRAWAL_AMOUNT))?
            .to_u128();
        Ok(PrepareWithdrawalRecord {
            withdrawal_lock_hash,
            block_number,
            amount,
        })
    }

    /// Remove a SUDT prepare withdrawal record
    fn remove_prepare_withdrawal(&mut self, sudt_id: u32, id: u32) -> Result<(), Error> {
        self.update_value(
            sudt_id,
            &build_sudt_field_key(id, WITHDRAWAL_LOCK_HASH),
            H256::zero(),
        )?;
        self.update_value(
            sudt_id,
            &build_sudt_field_key(id, WITHDRAWAL_BLOCK_NUMBER),
            H256::zero(),
        )?;
        self.update_value(
            sudt_id,
            &build_sudt_field_key(id, WITHDRAWAL_AMOUNT),
            H256::zero(),
        )?;
        Ok(())
    }
}
