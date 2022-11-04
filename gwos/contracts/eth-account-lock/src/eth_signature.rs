//! Secp256k1 Eth implementation

use gw_utils::{ckb_std::debug, error::Error, gw_common::H256, gw_types::bytes::Bytes};
use secp256k1_utils::recover_uncompressed_key;
use sha3::{Digest, Keccak256};

pub type EthAddress = [u8; 20];

pub fn extract_eth_lock_args(lock_args: Bytes) -> Result<(H256, EthAddress), Error> {
    if lock_args.len() != 52 {
        debug!("Invalid lock args len: {}", lock_args.len());
        return Err(Error::InvalidArgs);
    }
    let rollup_script_hash = {
        let mut buf = [0u8; 32];
        buf.copy_from_slice(&lock_args[..32]);
        buf.into()
    };
    let eth_address = {
        let mut buf = [0u8; 20];
        buf.copy_from_slice(&lock_args[32..]);
        buf
    };
    Ok((rollup_script_hash, eth_address))
}

#[derive(Default)]
pub struct Secp256k1Eth;

impl Secp256k1Eth {
    pub fn verify_alone(
        &self,
        eth_address: EthAddress,
        signature: [u8; 65],
        message: H256,
    ) -> Result<bool, Error> {
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
        if pubkey_hash != eth_address {
            return Ok(false);
        }
        Ok(true)
    }

    pub fn verify_message(
        &self,
        eth_address: EthAddress,
        signature: [u8; 65],
        message: H256,
    ) -> Result<bool, Error> {
        let mut hasher = Keccak256::new();
        hasher.update("\x19Ethereum Signed Message:\n32");
        hasher.update(message.as_slice());
        let buf = hasher.finalize();
        let mut signing_message = [0u8; 32];
        signing_message.copy_from_slice(&buf[..]);
        let signing_message = H256::from(signing_message);

        self.verify_alone(eth_address, signature, signing_message)
    }
}
