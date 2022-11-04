//! Secp256k1 Eth implementation

use gw_utils::{ckb_std::debug, error::Error, gw_common::H256, gw_types::bytes::Bytes};
use secp256k1_utils::recover_uncompressed_key;
use sha3::{Digest, Keccak256};

pub type TronAddress = [u8; 20];

pub fn extract_lock_args(lock_args: Bytes) -> Result<(H256, TronAddress), Error> {
    if lock_args.len() != 52 {
        debug!("Invalid lock args len: {}", lock_args.len());
        return Err(Error::InvalidArgs);
    }
    let rollup_script_hash = {
        let mut buf = [0u8; 32];
        buf.copy_from_slice(&lock_args[..32]);
        buf.into()
    };
    let address = {
        let mut buf = [0u8; 20];
        buf.copy_from_slice(&lock_args[32..]);
        buf
    };
    Ok((rollup_script_hash, address))
}

#[derive(Default)]
pub struct Secp256k1Tron;

impl Secp256k1Tron {
    pub fn verify_alone(
        &self,
        address: TronAddress,
        mut signature: [u8; 65],
        message: H256,
    ) -> Result<bool, Error> {
        // rewrite rec_id
        signature[64] = match signature[64] {
            28 => 1,
            _ => 0,
        };
        let pubkey = recover_uncompressed_key(message.into(), signature).map_err(|err| {
            debug!("failed to recover secp256k1 pubkey, error number: {}", err);
            Error::WrongSignature
        })?;
        let pubkey_hash = {
            let mut hasher = Keccak256::new();
            hasher.update(&pubkey[1..]);
            let buf = hasher.finalize();
            let mut pubkey_hash = [0u8; 20];
            pubkey_hash.copy_from_slice(&buf[12..]);
            pubkey_hash
        };
        if pubkey_hash != address {
            return Ok(false);
        }
        Ok(true)
    }

    pub fn verify_message(
        &self,
        address: TronAddress,
        signature: [u8; 65],
        message: H256,
    ) -> Result<bool, Error> {
        let mut hasher = Keccak256::new();
        hasher.update("\x19TRON Signed Message:\n32");
        hasher.update(message.as_slice());
        let buf = hasher.finalize();
        let mut signing_message = [0u8; 32];
        signing_message.copy_from_slice(&buf[..]);
        let signing_message = H256::from(signing_message);

        self.verify_alone(address, signature, signing_message)
    }
}
