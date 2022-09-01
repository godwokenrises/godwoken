use std::convert::TryInto;

use super::eip712::types::EIP712Domain;
use super::LockAlgorithm;
use crate::account_lock_manage::eip712::traits::EIP712Encode;
use crate::account_lock_manage::eip712::types::Withdrawal;
use crate::error::LockAlgorithmError;
use anyhow::bail;
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
pub struct Secp256k1Eth;

impl Secp256k1Eth {
    pub fn polyjuice_tx_signing_message(
        chain_id: u64,
        raw_tx: &RawL2Transaction,
        receiver_script: &Script,
    ) -> anyhow::Result<H256> {
        let tx_chain_id = raw_tx.chain_id().unpack();
        let is_protected = raw_tx.is_chain_id_protected();
        if is_protected && chain_id != tx_chain_id {
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

    pub fn domain_with_chain_id(chain_id: u64) -> EIP712Domain {
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

/// Usage
/// register AlwaysSuccess to AccountLockManage
///
/// manage.register_lock_algorithm(code_hash, Box::new(AlwaysSuccess::default()));
impl LockAlgorithm for Secp256k1Eth {
    fn recover(&self, message: H256, signature: &[u8]) -> Result<Bytes, LockAlgorithmError> {
        // extract rec_id
        fn extract_rec_id(rec_id: u8) -> u8 {
            match rec_id {
                r if r == 27 => 0,
                r if r == 28 => 1,
                r => r,
            }
        }

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
        let expected_chain_id = ctx.rollup_config.chain_id().unpack();
        let chain_id = tx.raw().chain_id().unpack();
        // Non EIP-155 transaction's chain_id is zero.
        // We support non EIP-155 for the compatibility.
        // Related issue: https://github.com/nervosnetwork/godwoken/issues/775
        let is_protected = tx.raw().is_chain_id_protected();
        // check protected chain id
        if is_protected && expected_chain_id != chain_id {
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

        // Try verify transaction with EIP-712 message
        // Reject transaction without chain_id protection
        if !is_protected {
            return Err(LockAlgorithmError::InvalidTransactionArgs);
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
        ctx: &RollupContext,
        sender_script: Script,
        withdrawal: &WithdrawalRequestExtra,
        address: RegistryAddress,
    ) -> Result<(), LockAlgorithmError> {
        let expected_chain_id = ctx.rollup_config.chain_id().unpack();
        let chain_id = withdrawal.raw().chain_id().unpack();
        if expected_chain_id != chain_id {
            return Err(LockAlgorithmError::InvalidSignature(format!(
                "Invalid chain id {} expected {}",
                chain_id, expected_chain_id
            )));
        }
        let typed_message = Withdrawal::from_raw(
            withdrawal.raw(),
            withdrawal.owner_lock(),
            address,
        )
        .map_err(|err| {
            LockAlgorithmError::InvalidSignature(format!("Invalid withdrawal format {}", err))
        })?;
        let message =
            typed_message.eip712_message(Self::domain_with_chain_id(chain_id).hash_struct());
        self.verify_alone(
            sender_script.args().unpack(),
            withdrawal.request().signature().unpack(),
            message.into(),
        )?;
        Ok(())
    }
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
    } else if parser.is_native_transfer() {
        if let Some(to_address) = parser.to_address() {
            to_address.to_vec()
        } else {
            log::error!("Invalid native token transfer transaction, [to_address] isn't set.");
            return None;
        }
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
    let is_protected = raw_tx.is_chain_id_protected();
    // EIP-155 - https://eips.ethereum.org/EIPS/eip-155
    if is_protected {
        stream.append(&raw_tx.chain_id().unpack());
        stream.append(&0u8);
        stream.append(&0u8);
    }
    stream.finalize_unbounded_list();
    Some(Bytes::from(stream.out().to_vec()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use gw_common::builtins::{CKB_SUDT_ACCOUNT_ID, ETH_REGISTRY_ACCOUNT_ID};
    use gw_types::{core::ScriptHashType, packed::RollupConfig};

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
    fn test_secp256k1_eth_polyjuice_native_token_transfer() {
        let chain_id = 42;
        let mut polyjuice_args = vec![0u8; 72];
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
        let to_address = [3u8; 20];
        polyjuice_args[52..].copy_from_slice(&to_address);

        let to_id: u32 = 4;
        let raw_tx = RawL2Transaction::new_builder()
            .chain_id(chain_id.pack())
            .from_id(0u32.pack())
            .to_id(to_id.pack())
            .nonce(0u32.pack())
            .args(polyjuice_args.pack())
            .build();
        let mut signature = [0u8; 65];
        signature.copy_from_slice(&hex::decode("58810245d67f0bde7961bcf03c1c7c54d1164b612f88e63e847ea693aad92fc32bb7680f39dc0dec403ba0b4eb6340cd2ad209448720133377cfdb8acd383b8001").unwrap());
        let tx = L2Transaction::new_builder()
            .raw(raw_tx)
            .signature(signature.to_vec().pack())
            .build();

        let rollup_type_hash =
            hex::decode("77c93b0632b5b6c3ef922c5b7cea208fb0a7c427a13d50e13d3fefad17e0c590")
                .unwrap();

        let mut args = rollup_type_hash.as_slice().to_vec();
        args.extend_from_slice(&CKB_SUDT_ACCOUNT_ID.to_le_bytes());

        let mut sender_args = vec![];
        let sender_eth_addr = hex::decode("5d200e1316687546fc6888259609c9aee0691f59").unwrap();
        let sender_reg_addr = RegistryAddress::new(ETH_REGISTRY_ACCOUNT_ID, sender_eth_addr);
        sender_args.extend(&rollup_type_hash);
        sender_args.extend(&sender_reg_addr.address);
        let sender_script = Script::new_builder()
            .args(Bytes::from(sender_args).pack())
            .build();
        let mock_polyjuice_code_hash = [0u8; 32];
        let receive_script = Script::new_builder()
            .code_hash(mock_polyjuice_code_hash.pack())
            .hash_type(ScriptHashType::Type.into())
            .args(args.pack())
            .build();
        let ctx = RollupContext {
            rollup_script_hash: Default::default(),
            rollup_config: RollupConfig::new_builder()
                .chain_id(chain_id.pack())
                .build(),
        };
        let eth = Secp256k1Eth::default();
        eth.verify_tx(&ctx, sender_reg_addr, sender_script, receive_script, tx)
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
    fn test_non_eip_155() {
        // This test case uses the deployment transaction from https://eips.ethereum.org/EIPS/eip-1820
        let mut args = vec![0u8; 52];
        args[0..7].copy_from_slice(b"\xFF\xFF\xFFPOLY");
        // eth create
        args[7] = 3;
        // gas
        args[8..16].copy_from_slice(&800000u64.to_le_bytes());
        // gasprice
        args[16..32].copy_from_slice(&100000000000u128.to_le_bytes());
        // value
        args[32..48].copy_from_slice(&0u128.to_le_bytes());
        // data size
        let data = hex::decode("608060405234801561001057600080fd5b506109c5806100206000396000f3fe608060405234801561001057600080fd5b50600436106100a5576000357c010000000000000000000000000000000000000000000000000000000090048063a41e7d5111610078578063a41e7d51146101d4578063aabbb8ca1461020a578063b705676514610236578063f712f3e814610280576100a5565b806329965a1d146100aa5780633d584063146100e25780635df8122f1461012457806365ba36c114610152575b600080fd5b6100e0600480360360608110156100c057600080fd5b50600160a060020a038135811691602081013591604090910135166102b6565b005b610108600480360360208110156100f857600080fd5b5035600160a060020a0316610570565b60408051600160a060020a039092168252519081900360200190f35b6100e06004803603604081101561013a57600080fd5b50600160a060020a03813581169160200135166105bc565b6101c26004803603602081101561016857600080fd5b81019060208101813564010000000081111561018357600080fd5b82018360208201111561019557600080fd5b803590602001918460018302840111640100000000831117156101b757600080fd5b5090925090506106b3565b60408051918252519081900360200190f35b6100e0600480360360408110156101ea57600080fd5b508035600160a060020a03169060200135600160e060020a0319166106ee565b6101086004803603604081101561022057600080fd5b50600160a060020a038135169060200135610778565b61026c6004803603604081101561024c57600080fd5b508035600160a060020a03169060200135600160e060020a0319166107ef565b604080519115158252519081900360200190f35b61026c6004803603604081101561029657600080fd5b508035600160a060020a03169060200135600160e060020a0319166108aa565b6000600160a060020a038416156102cd57836102cf565b335b9050336102db82610570565b600160a060020a031614610339576040805160e560020a62461bcd02815260206004820152600f60248201527f4e6f7420746865206d616e616765720000000000000000000000000000000000604482015290519081900360640190fd5b6103428361092a565b15610397576040805160e560020a62461bcd02815260206004820152601a60248201527f4d757374206e6f7420626520616e204552433136352068617368000000000000604482015290519081900360640190fd5b600160a060020a038216158015906103b85750600160a060020a0382163314155b156104ff5760405160200180807f455243313832305f4143434550545f4d4147494300000000000000000000000081525060140190506040516020818303038152906040528051906020012082600160a060020a031663249cb3fa85846040518363ffffffff167c01000000000000000000000000000000000000000000000000000000000281526004018083815260200182600160a060020a0316600160a060020a031681526020019250505060206040518083038186803b15801561047e57600080fd5b505afa158015610492573d6000803e3d6000fd5b505050506040513d60208110156104a857600080fd5b5051146104ff576040805160e560020a62461bcd02815260206004820181905260248201527f446f6573206e6f7420696d706c656d656e742074686520696e74657266616365604482015290519081900360640190fd5b600160a060020a03818116600081815260208181526040808320888452909152808220805473ffffffffffffffffffffffffffffffffffffffff19169487169485179055518692917f93baa6efbd2244243bfee6ce4cfdd1d04fc4c0e9a786abd3a41313bd352db15391a450505050565b600160a060020a03818116600090815260016020526040812054909116151561059a5750806105b7565b50600160a060020a03808216600090815260016020526040902054165b919050565b336105c683610570565b600160a060020a031614610624576040805160e560020a62461bcd02815260206004820152600f60248201527f4e6f7420746865206d616e616765720000000000000000000000000000000000604482015290519081900360640190fd5b81600160a060020a031681600160a060020a0316146106435780610646565b60005b600160a060020a03838116600081815260016020526040808220805473ffffffffffffffffffffffffffffffffffffffff19169585169590951790945592519184169290917f605c2dbf762e5f7d60a546d42e7205dcb1b011ebc62a61736a57c9089d3a43509190a35050565b600082826040516020018083838082843780830192505050925050506040516020818303038152906040528051906020012090505b92915050565b6106f882826107ef565b610703576000610705565b815b600160a060020a03928316600081815260208181526040808320600160e060020a031996909616808452958252808320805473ffffffffffffffffffffffffffffffffffffffff19169590971694909417909555908152600284528181209281529190925220805460ff19166001179055565b600080600160a060020a038416156107905783610792565b335b905061079d8361092a565b156107c357826107ad82826108aa565b6107b85760006107ba565b815b925050506106e8565b600160a060020a0390811660009081526020818152604080832086845290915290205416905092915050565b6000808061081d857f01ffc9a70000000000000000000000000000000000000000000000000000000061094c565b909250905081158061082d575080155b1561083d576000925050506106e8565b61084f85600160e060020a031961094c565b909250905081158061086057508015155b15610870576000925050506106e8565b61087a858561094c565b909250905060018214801561088f5750806001145b1561089f576001925050506106e8565b506000949350505050565b600160a060020a0382166000908152600260209081526040808320600160e060020a03198516845290915281205460ff1615156108f2576108eb83836107ef565b90506106e8565b50600160a060020a03808316600081815260208181526040808320600160e060020a0319871684529091529020549091161492915050565b7bffffffffffffffffffffffffffffffffffffffffffffffffffffffff161590565b6040517f01ffc9a7000000000000000000000000000000000000000000000000000000008082526004820183905260009182919060208160248189617530fa90519096909550935050505056fea165627a7a72305820377f4a2d4301ede9949f163f319021a6e9c687c292a5e2b2c4734c126b524e6c0029").unwrap();
        args[48..52].copy_from_slice(&(data.len() as u32).to_le_bytes());
        args.extend(data);
        let raw_tx = RawL2Transaction::new_builder()
            .nonce(0u32.pack())
            .args(args.pack())
            .build();
        let mut signature = [0u8; 65];
        signature[64] = 27;
        signature[0..32].copy_from_slice(
            &hex::decode("1820182018201820182018201820182018201820182018201820182018201820")
                .unwrap(),
        );
        signature[32..64].copy_from_slice(
            &hex::decode("1820182018201820182018201820182018201820182018201820182018201820")
                .unwrap(),
        );
        let tx = L2Transaction::new_builder()
            .raw(raw_tx)
            .signature(signature.pack())
            .build();
        // sender
        let sender_address = RegistryAddress::new(
            ETH_REGISTRY_ACCOUNT_ID,
            hex::decode("a990077c3205cbDf861e17Fa532eeB069cE9fF96").expect("hex decode"),
        );
        let rollup_type_hash = vec![0u8; 32];
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
            rollup_config: RollupConfig::new_builder().chain_id(0.pack()).build(),
        };
        let eth = Secp256k1Eth::default();
        eth.verify_tx(&ctx, sender_address, sender_script, receiver_script, tx)
            .expect("verify signature");
    }
}
