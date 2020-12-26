use super::LockAlgorithm;
use crate::error::LockAlgorithmError;
use gw_common::blake2b::new_blake2b;
use gw_common::H256;
use gw_types::prelude::*;
use gw_types::{bytes::Bytes, packed::Signature};
use lazy_static::lazy_static;
use secp256k1::recovery::{RecoverableSignature, RecoveryId};

lazy_static! {
    pub static ref SECP256K1: secp256k1::Secp256k1<secp256k1::All> = secp256k1::Secp256k1::new();
}

#[derive(Debug, Default)]
pub struct Secp256k1;

/// Usage
/// register AlwaysSuccess to AccountLockManage
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
        if &pubkey_hash != &expected_pubkey_hash {
            return Ok(false);
        }
        Ok(true)
    }
}
