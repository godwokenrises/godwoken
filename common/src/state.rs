use crate::blake2b::new_blake2b;
use crate::smt::Error as SMTError;
use core::mem::size_of;

/* Key type */
pub const GW_ACCOUNT_KV: u8 = 0;
pub const GW_ACCOUNT_NONCE: u8 = 1;
pub const GW_ACCOUNT_PUBKEY_HASH: u8 = 2;
pub const GW_ACCOUNT_CODE_HASH: u8 = 3;

pub const ZERO: [u8; 32] = [0u8; 32];

/* Generate raw key
 * raw_key: blake2b(id | type | key)
 *
 * We use raw key in the underlying KV store
 */
fn build_raw_key(id: u32, key: &[u8]) -> [u8; 32] {
    let mut raw_key = ZERO;
    let mut hasher = new_blake2b();
    hasher.update(&id.to_le_bytes());
    hasher.update(&[GW_ACCOUNT_KV]);
    hasher.update(key);
    hasher.finalize(&mut raw_key);
    raw_key
}

fn build_account_key(id: u32, type_: u8) -> [u8; 32] {
    let mut key = ZERO;
    key[..size_of::<u32>()].copy_from_slice(&id.to_le_bytes());
    key[size_of::<u32>()] = type_;
    key
}

fn generate_sudt_key(token_id: &[u8; 32], id: u32) -> [u8; 32] {
    // build application key
    let mut buf = ZERO;
    let mut hasher = new_blake2b();
    hasher.update(token_id);
    hasher.update(&id.to_le_bytes());
    hasher.finalize(&mut buf);
    buf
}

#[derive(Debug, Eq, PartialEq, Clone)]
pub enum Error {
    SMT(SMTError),
    AmountOverflow,
    MerkleProof,
}

impl From<SMTError> for Error {
    fn from(err: SMTError) -> Self {
        Error::SMT(err)
    }
}

pub trait State {
    // KV interface
    fn get_raw(&self, key: &[u8; 32]) -> Result<[u8; 32], Error>;
    fn update_raw(&mut self, key: [u8; 32], value: [u8; 32]) -> Result<(), Error>;
    fn calculate_root(&self) -> Result<[u8; 32], Error>;

    // implementations
    fn get_value(&self, id: u32, key: &[u8]) -> Result<[u8; 32], Error> {
        let raw_key = build_raw_key(id, key);
        self.get_raw(&raw_key)
    }
    fn update_value(&mut self, id: u32, key: &[u8], value: [u8; 32]) -> Result<(), Error> {
        let raw_key = build_raw_key(id, key);
        self.update_raw(raw_key, value)?;
        Ok(())
    }
    /// Create a new account
    fn create_account(
        &mut self,
        id: u32,
        code_hash: [u8; 32],
        pubkey_hash: [u8; 20],
    ) -> Result<(), Error> {
        self.update_raw(build_account_key(id, GW_ACCOUNT_NONCE).into(), ZERO)?;
        self.update_raw(
            build_account_key(id, GW_ACCOUNT_CODE_HASH).into(),
            code_hash.into(),
        )?;
        let mut pubkey_hash_value = ZERO;
        pubkey_hash_value[..pubkey_hash.len()].copy_from_slice(&pubkey_hash);
        self.update_raw(
            build_account_key(id, GW_ACCOUNT_PUBKEY_HASH).into(),
            pubkey_hash_value.into(),
        )?;
        Ok(())
    }
    fn get_code_hash(&self, id: u32) -> Result<[u8; 32], Error> {
        let value = self.get_raw(&build_account_key(id, GW_ACCOUNT_CODE_HASH).into())?;
        Ok(value.into())
    }
    fn get_nonce(&self, id: u32) -> Result<u32, Error> {
        let value = self.get_raw(&build_account_key(id, GW_ACCOUNT_NONCE).into())?;
        let mut nonce_bytes = [0u8; 4];
        nonce_bytes.copy_from_slice(&value[..4]);
        Ok(u32::from_le_bytes(nonce_bytes))
    }
    fn get_pubkey_hash(&self, id: u32) -> Result<[u8; 20], Error> {
        let value = self.get_raw(&build_account_key(id, GW_ACCOUNT_PUBKEY_HASH).into())?;
        let mut pubkey_hash = [0u8; 20];
        pubkey_hash.copy_from_slice(&value[..20]);
        Ok(pubkey_hash)
    }

    fn get_sudt_balance(&self, token_id: &[u8; 32], id: u32) -> Result<u128, Error> {
        let key = generate_sudt_key(token_id, id);
        // get balance
        let balance = {
            let v = self.get_value(id, &key)?;
            let mut buf = [0u8; 16];
            buf.copy_from_slice(&v[..16]);
            u128::from_le_bytes(buf)
        };
        Ok(balance)
    }

    fn mint_sudt(&mut self, token_id: &[u8; 32], id: u32, amount: u128) -> Result<(), Error> {
        let raw_key = build_raw_key(id, &generate_sudt_key(token_id, id));
        // calculate balance
        let mut balance = {
            let v = self.get_raw(&raw_key)?;
            let mut buf = [0u8; 16];
            buf.copy_from_slice(&v[..16]);
            u128::from_le_bytes(buf)
        };

        balance = balance.checked_add(amount).ok_or(Error::AmountOverflow)?;
        let mut value = ZERO;
        value[..16].copy_from_slice(&balance.to_le_bytes());
        self.update_raw(raw_key, value.into())?;
        Ok(())
    }
}
