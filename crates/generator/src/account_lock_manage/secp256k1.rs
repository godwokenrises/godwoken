use super::LockAlgorithm;
use crate::{error::LockAlgorithmError, RollupContext};
use gw_common::blake2b::new_blake2b;
use gw_common::H256;
use gw_types::prelude::*;
use gw_types::{
    bytes::Bytes,
    packed::{L2Transaction, Script, Signature},
};
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
    fn verify_tx(
        &self,
        ctx: &RollupContext,
        sender_script: Script,
        receiver_script: Script,
        tx: &L2Transaction,
    ) -> Result<bool, LockAlgorithmError> {
        let message = calc_godwoken_signing_message(
            &ctx.rollup_script_hash,
            &sender_script,
            &receiver_script,
            tx,
        );

        self.verify_message(sender_script.args().unpack(), tx.signature(), message)
    }

    fn verify_message(
        &self,
        lock_args: Bytes,
        signature: Signature,
        message: H256,
    ) -> Result<bool, LockAlgorithmError> {
        if lock_args.len() != 52 {
            return Err(LockAlgorithmError::InvalidLockArgs);
        }
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

impl Secp256k1Eth {
    fn verify_alone(
        &self,
        lock_args: Bytes,
        signature: Signature,
        message: H256,
    ) -> Result<bool, LockAlgorithmError> {
        if lock_args.len() != 52 {
            return Err(LockAlgorithmError::InvalidLockArgs);
        }

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
        let msg = secp256k1::Message::from_slice(message.as_slice())
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

/// Usage
/// register AlwaysSuccess to AccountLockManage
///
/// manage.register_lock_algorithm(code_hash, Box::new(AlwaysSuccess::default()));
impl LockAlgorithm for Secp256k1Eth {
    fn verify_tx(
        &self,
        ctx: &RollupContext,
        sender_script: Script,
        receiver_script: Script,
        tx: &L2Transaction,
    ) -> Result<bool, LockAlgorithmError> {
        let message = calc_godwoken_signing_message(
            &ctx.rollup_script_hash,
            &sender_script,
            &receiver_script,
            &tx,
        );
        self.verify_message(sender_script.args().unpack(), tx.signature(), message)
    }

    // NOTE: verify_mesage here is using Ethereum's
    // personal sign(with "\x19Ethereum Signed Message:\n32" appended)
    fn verify_message(
        &self,
        lock_args: Bytes,
        signature: Signature,
        message: H256,
    ) -> Result<bool, LockAlgorithmError> {
        let mut hasher = Keccak256::new();
        hasher.update("\x19Ethereum Signed Message:\n32");
        hasher.update(message.as_slice());
        let buf = hasher.finalize();
        let mut signing_message = [0u8; 32];
        signing_message.copy_from_slice(&buf[..]);
        let signing_message = H256::from(signing_message);

        self.verify_alone(lock_args, signature, signing_message)
    }
}

#[derive(Debug, Default)]
pub struct Secp256k1Tron;

/// Usage
/// register Secp256k1Tron to AccountLockManage
///
/// manage.register_lock_algorithm(code_hash, Box::new(Secp256k1Tron::default()));
impl LockAlgorithm for Secp256k1Tron {
    fn verify_tx(
        &self,
        ctx: &RollupContext,
        sender_script: Script,
        receiver_script: Script,
        tx: &L2Transaction,
    ) -> Result<bool, LockAlgorithmError> {
        let message = calc_godwoken_signing_message(
            &ctx.rollup_script_hash,
            &sender_script,
            &receiver_script,
            &tx,
        );

        self.verify_message(sender_script.args().unpack(), tx.signature(), message)
    }

    fn verify_message(
        &self,
        lock_args: Bytes,
        signature: Signature,
        message: H256,
    ) -> Result<bool, LockAlgorithmError> {
        if lock_args.len() != 52 {
            return Err(LockAlgorithmError::InvalidLockArgs);
        }
        let mut hasher = Keccak256::new();
        hasher.update("\x19TRON Signed Message:\n32");
        hasher.update(message.as_slice());
        let buf = hasher.finalize();
        let mut signing_message = [0u8; 32];
        signing_message.copy_from_slice(&buf[..]);
        let signing_message = H256::from(signing_message);
        let mut expected_pubkey_hash = [0u8; 20];
        expected_pubkey_hash.copy_from_slice(&lock_args[32..52]);
        let signature: RecoverableSignature = {
            let signature: [u8; 65] = signature.unpack();
            let recid = {
                let rec_param: i32 = match signature[64] {
                    28 => 1,
                    _ => 0,
                };
                RecoveryId::from_i32(rec_param).map_err(|_| LockAlgorithmError::InvalidSignature)?
            };
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

fn calc_godwoken_signing_message(
    rollup_type_hash: &H256,
    sender_script: &Script,
    receiver_script: &Script,
    tx: &L2Transaction,
) -> H256 {
    tx.raw().calc_message(
        &rollup_type_hash,
        &sender_script.hash().into(),
        &receiver_script.hash().into(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_secp256k1_eth_withdrawal_signature() {
        let message = H256::from([0u8; 32]);
        let test_signature = Signature::from_slice(
        &hex::decode("c2ae67217b65b785b1add7db1e9deb1df2ae2c7f57b9c29de0dfc40c59ab8d47341a863876660e3d0142b71248338ed71d2d4eb7ca078455565733095ac25a5800").expect("hex decode"))
        .expect("create signature structure");
        let address = Bytes::from(
            hex::decode("ffafb3db9377769f5b59bfff6cd2cf942a34ab17").expect("hex decode"),
        );
        let mut lock_args = vec![0u8; 32];
        lock_args.extend(address);
        let eth = Secp256k1Eth {};
        let result = eth
            .verify_message(lock_args.into(), test_signature, message)
            .expect("verify signature");
        assert!(result);
    }

    #[test]
    fn test_secp256k1_tron() {
        let message = H256::from([0u8; 32]);
        let test_signature = Signature::from_slice(
        &hex::decode("702ec8cd52a61093519de11433595ee7177bc8beaef2836714efe23e01bbb45f7f4a51c079f16cc742a261fe53fa3d731704a7687054764d424bd92963a82a241b").expect("hex decode"))
        .expect("create signature structure");
        let address = Bytes::from(
            hex::decode("d0ebb370429e1cc8a7da1f7aeb2447083e15298b").expect("hex decode"),
        );
        let mut lock_args = vec![0u8; 32];
        lock_args.extend(address);
        let tron = Secp256k1Tron {};
        let result = tron
            .verify_message(lock_args.into(), test_signature, message)
            .expect("verify signature");
        assert!(result);
    }
}
