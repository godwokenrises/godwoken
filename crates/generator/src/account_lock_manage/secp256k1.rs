use std::convert::TryInto;

use super::eip712::types::EIP712Domain;
use super::LockAlgorithm;
use crate::account_lock_manage::eip712::traits::EIP712Encode;
use crate::account_lock_manage::eip712::types::Withdrawal;
use crate::error::LockAlgorithmError;
use anyhow::bail;
use gw_common::blake2b::new_blake2b;
use gw_common::registry_address::RegistryAddress;
use gw_common::H256;
use gw_types::offchain::RollupContext;
use gw_types::packed::WithdrawalRequestExtra;
use gw_types::prelude::*;
use gw_types::{
    bytes::Bytes,
    packed::{L2Transaction, RawL2Transaction, Script},
};
use gw_utils::polyjuice_parser::PolyjuiceParser;
use lazy_static::lazy_static;
use secp256k1::recovery::{RecoverableSignature, RecoveryId};
use sha3::{Digest, Keccak256};

lazy_static! {
    pub static ref SECP256K1: secp256k1::Secp256k1<secp256k1::All> = secp256k1::Secp256k1::new();
}

fn convert_signature_to_byte65(signature: &[u8]) -> Result<[u8; 65], LockAlgorithmError> {
    signature.try_into().map_err(|_| {
        LockAlgorithmError::InvalidSignature(format!(
            "Signature length is {}, expect 65",
            signature.len()
        ))
    })
}

#[derive(Debug, Default)]
pub struct Secp256k1;

impl Secp256k1 {
    fn verify_message(
        &self,
        lock_args: Bytes,
        signature: Bytes,
        message: H256,
    ) -> Result<(), LockAlgorithmError> {
        if lock_args.len() != 52 {
            return Err(LockAlgorithmError::InvalidLockArgs);
        }
        let pubkey_hash = self.recover(message, signature.as_ref())?;
        if pubkey_hash.as_ref() != &lock_args[32..52] {
            return Err(LockAlgorithmError::InvalidSignature(
                "Secp256k1: Mismatch pubkey hash".to_string(),
            ));
        }
        Ok(())
    }
}

/// Usage
/// register an algorithm to AccountLockManage
///
/// manage.register_lock_algorithm(code_hash, Box::new(AlwaysSuccess::default()));
impl LockAlgorithm for Secp256k1 {
    fn recover(&self, message: H256, signature: &[u8]) -> Result<Bytes, LockAlgorithmError> {
        let signature: RecoverableSignature = {
            let signature = convert_signature_to_byte65(signature)?;
            let recid = RecoveryId::from_i32(signature[64] as i32)
                .map_err(|err| LockAlgorithmError::InvalidSignature(err.to_string()))?;
            let data = &signature[..64];
            RecoverableSignature::from_compact(data, recid)
                .map_err(|err| LockAlgorithmError::InvalidSignature(err.to_string()))?
        };
        let msg = secp256k1::Message::from_slice(message.as_slice())
            .map_err(|err| LockAlgorithmError::InvalidSignature(err.to_string()))?;
        let pubkey = SECP256K1
            .recover(&msg, &signature)
            .map_err(|err| LockAlgorithmError::InvalidSignature(err.to_string()))?;

        let mut buf = [0u8; 32];
        let mut hasher = new_blake2b();
        hasher.update(&pubkey.serialize());
        hasher.finalize(&mut buf);
        Ok(Bytes::copy_from_slice(&buf[..20]))
    }

    fn verify_tx(
        &self,
        ctx: &RollupContext,
        _sender: RegistryAddress,
        sender_script: Script,
        receiver_script: Script,
        tx: L2Transaction,
    ) -> Result<(), LockAlgorithmError> {
        let message = calc_godwoken_signing_message(
            &ctx.rollup_script_hash,
            &sender_script,
            &receiver_script,
            &tx,
        );

        self.verify_message(
            sender_script.args().unpack(),
            tx.signature().unpack(),
            message,
        )
    }

    fn verify_withdrawal(
        &self,
        sender_script: Script,
        withdrawal: &WithdrawalRequestExtra,
        _address: RegistryAddress,
    ) -> Result<(), LockAlgorithmError> {
        let lock_args: Bytes = sender_script.args().unpack();
        let message = withdrawal.raw().hash().into();
        let signature: Bytes = withdrawal.request().signature().unpack();
        self.verify_message(lock_args, signature, message)
    }
}

#[derive(Debug, Default)]
pub struct Secp256k1Eth;

impl Secp256k1Eth {
    pub fn polyjuice_tx_signing_message(
        chain_id: u64,
        raw_tx: &RawL2Transaction,
        receiver_script: &Script,
    ) -> anyhow::Result<H256> {
        let tx_chain_id = raw_tx.chain_id().unpack();
        if chain_id != tx_chain_id {
            bail!("mismatch tx chain id");
        }

        let rlp_data = try_assemble_polyjuice_args(raw_tx, receiver_script)
            .ok_or_else(|| anyhow::anyhow!("invalid polyjuice args"))?;

        let mut hasher = Keccak256::new();
        hasher.update(&rlp_data);
        let signing_message: [u8; 32] = hasher.finalize().into();

        Ok(signing_message.into())
    }

    pub fn eip712_signing_message(
        chain_id: u64,
        raw_tx: &RawL2Transaction,
        sender_registry_address: RegistryAddress,
        to_script_hash: H256,
    ) -> anyhow::Result<H256> {
        let typed_tx = crate::account_lock_manage::eip712::types::L2Transaction::from_raw(
            raw_tx,
            sender_registry_address,
            to_script_hash,
        )?;
        let message = typed_tx.eip712_message(Self::domain_with_chain_id(chain_id).hash_struct());

        Ok(message.into())
    }

    fn domain_with_chain_id(chain_id: u64) -> EIP712Domain {
        EIP712Domain {
            name: "Godwoken".to_string(),
            chain_id,
            version: "1".to_string(),
            verifying_contract: None,
            salt: None,
        }
    }

    fn verify_alone(
        &self,
        lock_args: Bytes,
        signature: Bytes,
        message: H256,
    ) -> Result<(), LockAlgorithmError> {
        if lock_args.len() != 52 {
            return Err(LockAlgorithmError::InvalidLockArgs);
        }

        let pubkey_hash = self.recover(message, signature.as_ref())?;
        if pubkey_hash.as_ref() != &lock_args[32..52] {
            return Err(LockAlgorithmError::InvalidSignature(
                "Secp256k1Eth: Mismatch pubkey hash".to_string(),
            ));
        }
        Ok(())
    }
}

// extract rec_id
fn extract_rec_id(rec_id: u8) -> u8 {
    match rec_id {
        r if r == 27 => 0,
        r if r == 28 => 1,
        r => r,
    }
}

/// Usage
/// register AlwaysSuccess to AccountLockManage
///
/// manage.register_lock_algorithm(code_hash, Box::new(AlwaysSuccess::default()));
impl LockAlgorithm for Secp256k1Eth {
    fn recover(&self, message: H256, signature: &[u8]) -> Result<Bytes, LockAlgorithmError> {
        let signature: RecoverableSignature = {
            let signature = convert_signature_to_byte65(signature)?;
            let recid = {
                let rec_param = extract_rec_id(signature[64]);
                RecoveryId::from_i32(rec_param.into())
                    .map_err(|err| LockAlgorithmError::InvalidSignature(err.to_string()))?
            };
            let data = &signature[..64];
            RecoverableSignature::from_compact(data, recid)
                .map_err(|err| LockAlgorithmError::InvalidSignature(err.to_string()))?
        };
        let msg = secp256k1::Message::from_slice(message.as_slice())
            .map_err(|err| LockAlgorithmError::InvalidSignature(err.to_string()))?;
        let pubkey = SECP256K1
            .recover(&msg, &signature)
            .map_err(|err| LockAlgorithmError::InvalidSignature(err.to_string()))?;

        let mut hasher = Keccak256::new();
        hasher.update(&pubkey.serialize_uncompressed()[1..]);
        let buf = hasher.finalize();
        Ok(Bytes::copy_from_slice(&buf[12..]))
    }

    fn verify_tx(
        &self,
        ctx: &RollupContext,
        sender_address: RegistryAddress,
        sender_script: Script,
        receiver_script: Script,
        tx: L2Transaction,
    ) -> Result<(), LockAlgorithmError> {
        // check chain id
        let expected_chain_id = ctx.rollup_config.chain_id().unpack();
        let chain_id = tx.raw().chain_id().unpack();
        if expected_chain_id != chain_id {
            return Err(LockAlgorithmError::InvalidTransactionArgs);
        }
        if let Some(rlp_data) = try_assemble_polyjuice_args(&tx.raw(), &receiver_script) {
            let mut hasher = Keccak256::new();
            hasher.update(&rlp_data);
            let signing_message: [u8; 32] = hasher.finalize().into();
            let signing_message = H256::from(signing_message);
            self.verify_alone(
                sender_script.args().unpack(),
                tx.signature().unpack(),
                signing_message,
            )?;
            return Ok(());
        }

        let raw_tx = tx.raw();
        let chain_id = raw_tx.chain_id().unpack();

        let to_script_hash = receiver_script.hash().into();

        let typed_tx = crate::account_lock_manage::eip712::types::L2Transaction::from_raw(
            &raw_tx,
            sender_address,
            to_script_hash,
        )
        .map_err(|err| {
            LockAlgorithmError::InvalidSignature(format!("Invalid l2 transaction format {}", err))
        })?;
        let message = typed_tx.eip712_message(Self::domain_with_chain_id(chain_id).hash_struct());
        self.verify_alone(
            sender_script.args().unpack(),
            tx.signature().unpack(),
            message.into(),
        )?;
        Ok(())
    }

    fn verify_withdrawal(
        &self,
        sender_script: Script,
        withdrawal: &WithdrawalRequestExtra,
        address: RegistryAddress,
    ) -> Result<(), LockAlgorithmError> {
        let typed_message = Withdrawal::from_raw(
            withdrawal.raw(),
            withdrawal.owner_lock(),
            address,
        )
        .map_err(|err| {
            LockAlgorithmError::InvalidSignature(format!("Invalid withdrawal format {}", err))
        })?;
        let message = typed_message.eip712_message(
            Self::domain_with_chain_id(withdrawal.raw().chain_id().unpack()).hash_struct(),
        );
        self.verify_alone(
            sender_script.args().unpack(),
            withdrawal.request().signature().unpack(),
            message.into(),
        )?;
        Ok(())
    }
}

#[derive(Debug, Default)]
pub struct Secp256k1Tron;

impl Secp256k1Tron {
    fn verify_message(
        &self,
        lock_args: Bytes,
        signature: Bytes,
        message: H256,
    ) -> Result<(), LockAlgorithmError> {
        if lock_args.len() != 52 {
            return Err(LockAlgorithmError::InvalidLockArgs);
        }
        let mut hasher = Keccak256::new();
        hasher.update("\x19TRON Signed Message:\n32");
        hasher.update(message.as_slice());
        let signing_message: [u8; 32] = hasher.finalize().into();
        let signing_message = H256::from(signing_message);
        let pubkey_hash = self.recover(signing_message, signature.as_ref())?;
        if pubkey_hash.as_ref() != &lock_args[32..52] {
            return Err(LockAlgorithmError::InvalidSignature(
                "Secp256k1Tron: Mismatch pubkey hash".to_string(),
            ));
        }
        Ok(())
    }
}

/// Usage
/// register Secp256k1Tron to AccountLockManage
///
/// manage.register_lock_algorithm(code_hash, Box::new(Secp256k1Tron::default()));
impl LockAlgorithm for Secp256k1Tron {
    fn recover(&self, message: H256, signature: &[u8]) -> Result<Bytes, LockAlgorithmError> {
        let signature: RecoverableSignature = {
            let signature: [u8; 65] = convert_signature_to_byte65(signature)?;
            let recid = {
                let rec_param = match signature[64] {
                    28 => 1,
                    _ => 0,
                };
                RecoveryId::from_i32(rec_param)
                    .map_err(|err| LockAlgorithmError::InvalidSignature(err.to_string()))?
            };
            let data = &signature[..64];
            RecoverableSignature::from_compact(data, recid)
                .map_err(|err| LockAlgorithmError::InvalidSignature(err.to_string()))?
        };
        let msg = secp256k1::Message::from_slice(message.as_slice())
            .map_err(|err| LockAlgorithmError::InvalidSignature(err.to_string()))?;
        let pubkey = SECP256K1
            .recover(&msg, &signature)
            .map_err(|err| LockAlgorithmError::InvalidSignature(err.to_string()))?;

        let mut hasher = Keccak256::new();
        hasher.update(&pubkey.serialize_uncompressed()[1..]);
        let buf = hasher.finalize();
        Ok(Bytes::copy_from_slice(&buf[12..]))
    }

    fn verify_tx(
        &self,
        ctx: &RollupContext,
        _sender: RegistryAddress,
        sender_script: Script,
        receiver_script: Script,
        tx: L2Transaction,
    ) -> Result<(), LockAlgorithmError> {
        let message = calc_godwoken_signing_message(
            &ctx.rollup_script_hash,
            &sender_script,
            &receiver_script,
            &tx,
        );

        self.verify_message(
            sender_script.args().unpack(),
            tx.signature().unpack(),
            message,
        )
    }

    fn verify_withdrawal(
        &self,
        sender_script: Script,
        withdrawal: &WithdrawalRequestExtra,
        _address: RegistryAddress,
    ) -> Result<(), LockAlgorithmError> {
        let message = withdrawal.request().raw().hash();
        self.verify_message(
            sender_script.args().unpack(),
            withdrawal.request().signature().unpack(),
            message.into(),
        )?;
        Ok(())
    }
}

fn calc_godwoken_signing_message(
    rollup_type_hash: &H256,
    sender_script: &Script,
    receiver_script: &Script,
    tx: &L2Transaction,
) -> H256 {
    tx.raw().calc_message(
        rollup_type_hash,
        &sender_script.hash().into(),
        &receiver_script.hash().into(),
    )
}
fn try_assemble_polyjuice_args(
    raw_tx: &RawL2Transaction,
    receiver_script: &Script,
) -> Option<Bytes> {
    let parser = PolyjuiceParser::from_raw_l2_tx(raw_tx)?;
    let mut stream = rlp::RlpStream::new();
    stream.begin_unbounded_list();
    let nonce: u32 = raw_tx.nonce().unpack();
    stream.append(&nonce);
    stream.append(&parser.gas_price());
    stream.append(&parser.gas());
    let to = if parser.is_create() {
        // 3 for EVMC_CREATE
        vec![0u8; 0]
    } else {
        // For contract calling, chain id is read from scrpit args of
        // receiver_script, see the following link for more details:
        // https://github.com/nervosnetwork/godwoken-polyjuice#normal-contract-account-script

        // NOTICE: this is a tempory solution, we should query it from state
        let args: Bytes = receiver_script.args().unpack();
        if args.len() != 56 {
            log::error!("invalid ETH contract script args len: {}", args.len());
            return None;
        }
        let to = args[36..].to_vec();
        assert_eq!(to.len(), 20, "eth address");
        to
    };
    stream.append(&to);
    stream.append(&parser.value());
    stream.append(&parser.data().to_vec());
    stream.append(&raw_tx.chain_id().unpack());
    stream.append(&0u8);
    stream.append(&0u8);
    stream.finalize_unbounded_list();
    Some(Bytes::from(stream.out().to_vec()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use gw_common::builtins::ETH_REGISTRY_ACCOUNT_ID;
    use gw_types::packed::RollupConfig;

    #[test]
    fn test_secp256k1_eth_polyjuice_call() {
        let mut polyjuice_args = vec![0u8; 52];
        polyjuice_args[0..7].copy_from_slice(b"\xFF\xFF\xFFPOLY");
        polyjuice_args[7] = 0;
        let gas_limit: u64 = 21000;
        polyjuice_args[8..16].copy_from_slice(&gas_limit.to_le_bytes());
        let gas_price: u128 = 20000000000;
        polyjuice_args[16..32].copy_from_slice(&gas_price.to_le_bytes());
        let value: u128 = 3000000;
        polyjuice_args[32..48].copy_from_slice(&value.to_le_bytes());
        let payload_length: u32 = 0;
        polyjuice_args[48..52].copy_from_slice(&payload_length.to_le_bytes());

        let chain_id = 23u64;

        let raw_tx = RawL2Transaction::new_builder()
            .chain_id(chain_id.pack())
            .nonce(9u32.pack())
            .to_id(1234u32.pack())
            .args(Bytes::from(polyjuice_args).pack())
            .build();
        let mut signature = [0u8; 65];
        signature.copy_from_slice(&hex::decode("87365fe2442f7773ebfe74963e7e09cd928cec7e9bc373d3dac901ca9fef16431c0ad18738a75de8c2844493c2b18d112146cebb3a2106f2edb41c4ec5f31d0c00").expect("hex decode"));
        let tx = L2Transaction::new_builder()
            .raw(raw_tx)
            .signature(signature.to_vec().pack())
            .build();
        let eth = Secp256k1Eth::default();

        let rollup_type_hash = vec![0u8; 32];

        let sender_address = RegistryAddress::new(
            ETH_REGISTRY_ACCOUNT_ID,
            hex::decode("0000a7ce68e7328ecf2c83b103b50c68cf60ae3a").expect("hex decode"),
        );
        let mut sender_args = vec![];
        sender_args.extend(&rollup_type_hash);
        sender_args.extend(&sender_address.address);
        let sender_script = Script::new_builder()
            .args(Bytes::from(sender_args).pack())
            .build();

        let mut receiver_args = vec![];
        receiver_args.extend(&rollup_type_hash);
        receiver_args.extend(&23u32.to_le_bytes());
        receiver_args.extend(&[42u8; 20]);
        let receiver_script = Script::new_builder()
            .args(Bytes::from(receiver_args).pack())
            .build();
        let ctx = RollupContext {
            rollup_script_hash: Default::default(),
            rollup_config: RollupConfig::new_builder()
                .chain_id(chain_id.pack())
                .build(),
        };
        eth.verify_tx(&ctx, sender_address, sender_script, receiver_script, tx)
            .expect("verify signature");
    }

    #[test]
    fn test_secp256k1_eth_polyjuice_call_with_to_containing_leading_zeros() {
        let mut polyjuice_args = vec![0u8; 52];
        polyjuice_args[0..7].copy_from_slice(b"\xFF\xFF\xFFPOLY");
        polyjuice_args[7] = 0;
        let gas_limit: u64 = 21000;
        polyjuice_args[8..16].copy_from_slice(&gas_limit.to_le_bytes());
        let gas_price: u128 = 20000000000;
        polyjuice_args[16..32].copy_from_slice(&gas_price.to_le_bytes());
        let value: u128 = 3000000;
        polyjuice_args[32..48].copy_from_slice(&value.to_le_bytes());
        let payload_length: u32 = 0;
        polyjuice_args[48..52].copy_from_slice(&payload_length.to_le_bytes());

        let chain_id = 23;
        let raw_tx = RawL2Transaction::new_builder()
            .chain_id(chain_id.pack())
            .nonce(9u32.pack())
            .to_id(1234u32.pack())
            .args(Bytes::from(polyjuice_args).pack())
            .build();
        let mut signature = [0u8; 65];
        signature.copy_from_slice(&hex::decode("3861489cb072a86a97a745f225dddef4885349169da8feb24a6439279c62da2862edb554be68bfeb1381b37392a2ae42df8591950c9a678f675c598c33a1ec3b00").expect("hex decode"));
        let tx = L2Transaction::new_builder()
            .raw(raw_tx)
            .signature(signature.to_vec().pack())
            .build();
        let eth = Secp256k1Eth::default();

        // This rollup type hash is used, so the receiver script hash is:
        // 00002b003de527c1d67f2a2a348683ecc9598647c30884c89c5dcf6da1afbddd,
        // which contains leading zeros to ensure RLP behavior.
        let rollup_type_hash =
            hex::decode("cfdefce91f70f53167971f74bf1074b6b889be270306aabd34e67404b75dacab")
                .expect("hex decode");

        let sender_address = RegistryAddress::new(
            ETH_REGISTRY_ACCOUNT_ID,
            hex::decode("0000A7CE68e7328eCF2C83b103b50C68CF60Ae3a").expect("hex decode"),
        );
        let mut sender_args = vec![];
        sender_args.extend(&rollup_type_hash);
        // Private key: dc88f509cab7f30ea36fd1aeb203403ce284e587bedecba73ba2fadf688acd19
        // Please do not use this private key elsewhere!
        sender_args.extend(&sender_address.address);
        let sender_script = Script::new_builder()
            .args(Bytes::from(sender_args).pack())
            .build();

        let mut receiver_args = vec![];
        receiver_args.extend(&rollup_type_hash);
        receiver_args.extend(&23u32.to_le_bytes());
        receiver_args.extend(&[11u8; 20]);
        let receiver_script = Script::new_builder()
            .args(Bytes::from(receiver_args).pack())
            .build();
        let ctx = RollupContext {
            rollup_script_hash: Default::default(),
            rollup_config: RollupConfig::new_builder()
                .chain_id(chain_id.pack())
                .build(),
        };

        eth.verify_tx(&ctx, sender_address, sender_script, receiver_script, tx)
            .expect("verify signature");
    }

    #[test]
    fn test_secp256k1_eth_polyjuice_create() {
        let mut polyjuice_args = vec![0u8; 69];
        polyjuice_args[0..7].copy_from_slice(b"\xFF\xFF\xFFPOLY");
        polyjuice_args[7] = 3;
        let gas_limit: u64 = 21000;
        polyjuice_args[8..16].copy_from_slice(&gas_limit.to_le_bytes());
        let gas_price: u128 = 20000000000;
        polyjuice_args[16..32].copy_from_slice(&gas_price.to_le_bytes());
        let value: u128 = 3000000;
        polyjuice_args[32..48].copy_from_slice(&value.to_le_bytes());
        let payload_length: u32 = 17;
        polyjuice_args[48..52].copy_from_slice(&payload_length.to_le_bytes());
        polyjuice_args[52..69].copy_from_slice(b"POLYJUICEcontract");

        let chain_id = 23;
        let raw_tx = RawL2Transaction::new_builder()
            .chain_id(chain_id.pack())
            .nonce(9u32.pack())
            .to_id(23u32.pack())
            .args(Bytes::from(polyjuice_args).pack())
            .build();
        let mut signature = [0u8; 65];
        signature.copy_from_slice(&hex::decode("5289a4c910f143a97ce6d8ce55a970863c115bb95b404518a183ec470734ce0c10594e911d54d8894d05381fbc0f052b7397cd25217f6f102d297387a4cb15d700").expect("hex decode"));
        let tx = L2Transaction::new_builder()
            .raw(raw_tx)
            .signature(signature.to_vec().pack())
            .build();
        let eth = Secp256k1Eth::default();

        let rollup_type_hash = vec![0u8; 32];

        let sender_address = RegistryAddress::new(
            ETH_REGISTRY_ACCOUNT_ID,
            hex::decode("9d8A62f656a8d1615C1294fd71e9CFb3E4855A4F").expect("hex decode"),
        );
        let mut sender_args = vec![];
        sender_args.extend(&rollup_type_hash);
        sender_args.extend(&sender_address.address);
        let sender_script = Script::new_builder()
            .args(Bytes::from(sender_args).pack())
            .build();

        let mut receiver_args = vec![];
        receiver_args.extend(&rollup_type_hash);
        receiver_args.extend(&23u32.to_le_bytes());
        let receiver_script = Script::new_builder()
            .args(Bytes::from(receiver_args).pack())
            .build();
        let ctx = RollupContext {
            rollup_script_hash: Default::default(),
            rollup_config: RollupConfig::new_builder()
                .chain_id(chain_id.pack())
                .build(),
        };
        eth.verify_tx(&ctx, sender_address, sender_script, receiver_script, tx)
            .expect("verify signature");
    }

    #[test]
    fn test_secp256k1_eth_normal_call() {
        let chain_id = 1u64;
        let raw_tx = RawL2Transaction::new_builder()
            .nonce(9u32.pack())
            .to_id(1234u32.pack())
            .chain_id(chain_id.pack())
            .build();
        let mut signature = [0u8; 65];
        signature.copy_from_slice(&hex::decode("64b164f5303000c283119974d7ba8f050cc7429984af904134d5cda6d3ce045934cc6b6f513ec939c2ae4cfb9cbee249ba8ae86f6274e4035c150f9c8e634a3a1b").expect("hex decode"));
        let tx = L2Transaction::new_builder()
            .raw(raw_tx)
            .signature(signature.to_vec().pack())
            .build();
        let eth = Secp256k1Eth::default();

        let rollup_type_hash = vec![0u8; 32];

        let sender_address = RegistryAddress::new(
            ETH_REGISTRY_ACCOUNT_ID,
            hex::decode("e8ae579256c3b84efb76bbb69cb6bcbef1375f00").expect("hex decode"),
        );
        let mut sender_args = vec![];
        sender_args.extend(&rollup_type_hash);
        sender_args.extend(&sender_address.address);
        let sender_script = Script::new_builder()
            .args(Bytes::from(sender_args).pack())
            .build();

        let mut receiver_args = vec![];
        receiver_args.extend(&rollup_type_hash);
        receiver_args.extend(&23u32.to_le_bytes());
        let receiver_script = Script::new_builder()
            .args(Bytes::from(receiver_args).pack())
            .build();
        let ctx = RollupContext {
            rollup_script_hash: Default::default(),
            rollup_config: RollupConfig::new_builder()
                .chain_id(chain_id.pack())
                .build(),
        };
        eth.verify_tx(&ctx, sender_address, sender_script, receiver_script, tx)
            .expect("verify signature");
    }

    #[test]
    fn test_secp256k1_tron() {
        let message = H256::from([0u8; 32]);
        let test_signature = Bytes::from(
        hex::decode("702ec8cd52a61093519de11433595ee7177bc8beaef2836714efe23e01bbb45f7f4a51c079f16cc742a261fe53fa3d731704a7687054764d424bd92963a82a241b").expect("hex decode"));
        let address = Bytes::from(
            hex::decode("d0ebb370429e1cc8a7da1f7aeb2447083e15298b").expect("hex decode"),
        );
        let mut lock_args = vec![0u8; 32];
        lock_args.extend(address);
        let tron = Secp256k1Tron {};
        tron.verify_message(lock_args.into(), test_signature, message)
            .expect("verify signature");
    }
}
