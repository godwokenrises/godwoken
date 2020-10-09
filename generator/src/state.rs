use crate::blake2b::new_blake2b;
use crate::smt::{SMTResult, Store, H256, SMT};
use crate::syscalls::RunResult;
use std::mem::size_of;

/* Key type */
const GW_ACCOUNT_KV: u8 = 0;
const GW_ACCOUNT_NONCE: u8 = 1;
const GW_ACCOUNT_PUBKEY_HASH: u8 = 2;
const GW_ACCOUNT_CODE_HASH: u8 = 3;

/* Generate raw key
 * raw_key: blake2b(id | type | key)
 *
 * We use raw key in the underlying KV store
 */
fn build_raw_key(id: u32, key: &[u8]) -> [u8; 32] {
    let mut raw_key = [0u8; 32];
    let mut hasher = new_blake2b();
    hasher.update(&id.to_le_bytes());
    hasher.update(&[GW_ACCOUNT_KV]);
    hasher.update(key);
    hasher.finalize(&mut raw_key);
    raw_key
}

fn build_account_key(id: u32, type_: u8) -> [u8; 32] {
    let mut key = [0u8; 32];
    key[..size_of::<u32>()].copy_from_slice(&id.to_le_bytes());
    key[size_of::<u32>()] = type_;
    key
}

pub trait State {
    fn update_state(&mut self, run_result: &RunResult) -> SMTResult<()>;
    fn get_value(&self, id: u32, key: &[u8]) -> SMTResult<[u8; 32]>;
    fn update_value(&mut self, id: u32, key: &[u8], value: [u8; 32]) -> SMTResult<()>;
    fn create_account(
        &mut self,
        id: u32,
        code_hash: [u8; 32],
        pubkey_hash: [u8; 20],
    ) -> SMTResult<()>;
    fn get_code_hash(&self, id: u32) -> SMTResult<[u8; 32]>;
    fn get_nonce(&self, id: u32) -> SMTResult<u32>;
    fn get_pubkey_hash(&self, id: u32) -> SMTResult<[u8; 20]>;
}

impl<S: Store<H256>> State for SMT<S> {
    fn update_state(&mut self, run_result: &RunResult) -> SMTResult<()> {
        for (k, v) in &run_result.write_values {
            self.update(*k, *v)?;
        }
        Ok(())
    }

    fn get_value(&self, id: u32, key: &[u8]) -> SMTResult<[u8; 32]> {
        let raw_key = build_raw_key(id, key);
        self.get(&raw_key.into()).map(Into::into)
    }

    fn update_value(&mut self, id: u32, key: &[u8], value: [u8; 32]) -> SMTResult<()> {
        let raw_key = build_raw_key(id, key);
        self.update(raw_key.into(), value.into())?;
        Ok(())
    }

    /// Create a new account
    fn create_account(
        &mut self,
        id: u32,
        code_hash: [u8; 32],
        pubkey_hash: [u8; 20],
    ) -> SMTResult<()> {
        self.update(build_account_key(id, GW_ACCOUNT_NONCE).into(), H256::zero())?;
        self.update(
            build_account_key(id, GW_ACCOUNT_CODE_HASH).into(),
            code_hash.into(),
        )?;
        let mut pubkey_hash_value = [0u8; 32];
        pubkey_hash_value[..pubkey_hash.len()].copy_from_slice(&pubkey_hash);
        self.update(
            build_account_key(id, GW_ACCOUNT_PUBKEY_HASH).into(),
            pubkey_hash_value.into(),
        )?;
        Ok(())
    }

    fn get_code_hash(&self, id: u32) -> SMTResult<[u8; 32]> {
        let value = self.get(&build_account_key(id, GW_ACCOUNT_CODE_HASH).into())?;
        Ok(value.into())
    }
    fn get_nonce(&self, id: u32) -> SMTResult<u32> {
        let value = self.get(&build_account_key(id, GW_ACCOUNT_NONCE).into())?;
        let mut nonce_bytes = [0u8; 4];
        nonce_bytes.copy_from_slice(&value.as_slice()[..4]);
        Ok(u32::from_le_bytes(nonce_bytes))
    }
    fn get_pubkey_hash(&self, id: u32) -> SMTResult<[u8; 20]> {
        let value = self.get(&build_account_key(id, GW_ACCOUNT_PUBKEY_HASH).into())?;
        let mut pubkey_hash = [0u8; 20];
        pubkey_hash.copy_from_slice(&value.as_slice()[..20]);
        Ok(pubkey_hash)
    }
}
