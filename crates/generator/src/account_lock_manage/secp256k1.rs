use super::LockAlgorithm;
use crate::error::LockAlgorithmError;
use gw_common::blake2b::new_blake2b;
use gw_common::H256;
use gw_types::prelude::*;
use gw_types::{bytes::Bytes, packed::Signature};
use lazy_static::lazy_static;
use secp256k1::recovery::{RecoverableSignature, RecoveryId};
use sha3::{Digest, Keccak256};

lazy_static! {
    pub static ref SECP256K1: secp256k1::Secp256k1<secp256k1::All> = secp256k1::Secp256k1::new();
}

#[derive(Debug, Default)]
pub struct Secp256k1;

/// Usage
/// register an algorithm to AccountLockManage
///
/// manage.register_lock_algorithm(code_hash, Box::new(AlwaysSuccess::default()));
impl LockAlgorithm for Secp256k1 {
    fn verify_signature(
        &self,
        lock_args: Bytes,
        signature: Signature,
        message: H256,
    ) -> Result<bool, LockAlgorithmError> {
        if lock_args.len() != 20 {
            return Err(LockAlgorithmError::InvalidLockArgs);
        }
        let mut expected_pubkey_hash = [0u8; 20];
        expected_pubkey_hash.copy_from_slice(&lock_args);
        let signature: RecoverableSignature = {
            let signature: [u8; 65] = signature.unpack();
            let recid = RecoveryId::from_i32(signature[64] as i32)
                .map_err(|_| LockAlgorithmError::InvalidSignature)?;
            let data = &signature[..64];
            RecoverableSignature::from_compact(data, recid)
                .map_err(|_| LockAlgorithmError::InvalidSignature)?
        };
        let msg = secp256k1::Message::from_slice(message.as_slice())
            .map_err(|_| LockAlgorithmError::InvalidSignature)?;
        let pubkey = SECP256K1
            .recover(&msg, &signature)
            .map_err(|_| LockAlgorithmError::InvalidSignature)?;
        let pubkey_hash = {
            let mut buf = [0u8; 32];
            let mut hasher = new_blake2b();
            hasher.update(&pubkey.serialize());
            hasher.finalize(&mut buf);
            let mut pubkey_hash = [0u8; 20];
            pubkey_hash.copy_from_slice(&buf[..20]);
            pubkey_hash
        };
        if pubkey_hash != expected_pubkey_hash {
            return Ok(false);
        }
        Ok(true)
    }
}

#[derive(Debug, Default)]
pub struct Secp256k1Eth;

/// Usage
/// register AlwaysSuccess to AccountLockManage
///
/// manage.register_lock_algorithm(code_hash, Box::new(AlwaysSuccess::default()));
impl LockAlgorithm for Secp256k1Eth {
    fn verify_signature(
        &self,
        lock_args: Bytes,
        signature: Signature,
        message: H256,
    ) -> Result<bool, LockAlgorithmError> {
        if lock_args.len() != 52 {
            return Err(LockAlgorithmError::InvalidLockArgs);
        }
        let mut hasher = Keccak256::new();
        hasher.update("\x19Ethereum Signed Message:\n32");
        hasher.update(message.as_slice());
        let buf = hasher.finalize();
        let mut signing_message = [0u8; 32];
        signing_message.copy_from_slice(&buf[..]);
        let signing_message = H256::from(signing_message);

        let mut expected_pubkey_hash = [0u8; 20];
        expected_pubkey_hash.copy_from_slice(&lock_args[32..52]);
        let signature: RecoverableSignature = {
            let signature: [u8; 65] = signature.unpack();
            let recid = RecoveryId::from_i32(signature[64] as i32)
                .map_err(|_| LockAlgorithmError::InvalidSignature)?;
            let data = &signature[..64];
            RecoverableSignature::from_compact(data, recid)
                .map_err(|_| LockAlgorithmError::InvalidSignature)?
        };
        let msg = secp256k1::Message::from_slice(signing_message.as_slice())
            .map_err(|_| LockAlgorithmError::InvalidSignature)?;
        let pubkey = SECP256K1
            .recover(&msg, &signature)
            .map_err(|_| LockAlgorithmError::InvalidSignature)?;
        let pubkey_hash = {
            let mut hasher = Keccak256::new();
            hasher.update(&pubkey.serialize_uncompressed()[1..]);
            let buf = hasher.finalize();
            let mut pubkey_hash = [0u8; 20];
            pubkey_hash.copy_from_slice(&buf[12..]);
            pubkey_hash
        };
        if pubkey_hash != expected_pubkey_hash {
            return Ok(false);
        }
        Ok(true)
    }
}

#[test]
fn test_secp256k1_eth() {
    let message = H256::from([0u8; 32]);
    let test_signature = Signature::from_slice(
        &hex::decode("c2ae67217b65b785b1add7db1e9deb1df2ae2c7f57b9c29de0dfc40c59ab8d47341a863876660e3d0142b71248338ed71d2d4eb7ca078455565733095ac25a5800").expect("hex decode"))
        .expect("create signature structure");
    let address =
        Bytes::from(hex::decode("ffafb3db9377769f5b59bfff6cd2cf942a34ab17").expect("hex decode"));
    let mut lock_args = vec![0u8; 32];
    lock_args.extend(address);
    let eth = Secp256k1Eth {};
    let result = eth
        .verify_signature(lock_args.into(), test_signature, message)
        .expect("verify signature");
    assert!(result);
}
