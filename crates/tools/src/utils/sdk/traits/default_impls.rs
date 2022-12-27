use std::collections::HashMap;

use ckb_crypto::secp::Pubkey;
use thiserror::Error;

use ckb_hash::blake2b_256;
use ckb_types::{
    bytes::Bytes,
    core::{BlockView, DepType, TransactionView},
    packed::{CellDep, CellOutput, OutPoint, Script},
    prelude::*,
    H160,
};

use super::OffchainCellDepResolver;
use crate::utils::sdk::traits::{CellDepResolver, Signer, SignerError};
use crate::utils::sdk::types::ScriptId;
use crate::utils::sdk::util::{serialize_signature, zeroize_privkey};
use crate::utils::sdk::{
    constants::{
        DAO_OUTPUT_LOC, DAO_TYPE_HASH, MULTISIG_GROUP_OUTPUT_LOC, MULTISIG_OUTPUT_LOC,
        MULTISIG_TYPE_HASH, SIGHASH_GROUP_OUTPUT_LOC, SIGHASH_OUTPUT_LOC, SIGHASH_TYPE_HASH,
    },
    util::keccak160,
};
use ckb_crypto::secp::SECP256K1;
use ckb_resource::{
    CODE_HASH_DAO, CODE_HASH_SECP256K1_BLAKE160_MULTISIG_ALL,
    CODE_HASH_SECP256K1_BLAKE160_SIGHASH_ALL,
};

/// Parse Genesis Info errors
#[derive(Error, Debug)]
pub enum ParseGenesisInfoError {
    #[error("invalid block number, expected: 0, got: `{0}`")]
    InvalidBlockNumber(u64),
    #[error("data not found: `{0}`")]
    DataHashNotFound(String),
    #[error("type not found: `{0}`")]
    TypeHashNotFound(String),
}

/// A cell_dep resolver use genesis info resolve system scripts and can register more cell_dep info.
#[derive(Clone)]
pub struct DefaultCellDepResolver {
    offchain: OffchainCellDepResolver,
}
impl DefaultCellDepResolver {
    pub fn from_genesis(
        genesis_block: &BlockView,
    ) -> Result<DefaultCellDepResolver, ParseGenesisInfoError> {
        let header = genesis_block.header();
        if header.number() != 0 {
            return Err(ParseGenesisInfoError::InvalidBlockNumber(header.number()));
        }
        let mut sighash_type_hash = None;
        let mut multisig_type_hash = None;
        let mut dao_type_hash = None;
        let out_points = genesis_block
            .transactions()
            .iter()
            .enumerate()
            .map(|(tx_index, tx)| {
                tx.outputs()
                    .into_iter()
                    .zip(tx.outputs_data().into_iter())
                    .enumerate()
                    .map(|(index, (output, data))| {
                        if tx_index == SIGHASH_OUTPUT_LOC.0 && index == SIGHASH_OUTPUT_LOC.1 {
                            sighash_type_hash = output
                                .type_()
                                .to_opt()
                                .map(|script| script.calc_script_hash());
                            let data_hash = CellOutput::calc_data_hash(&data.raw_data());
                            if data_hash != CODE_HASH_SECP256K1_BLAKE160_SIGHASH_ALL.pack() {
                                log::error!(
                                    "System sighash script code hash error! found: {}, expected: {}",
                                    data_hash,
                                    CODE_HASH_SECP256K1_BLAKE160_SIGHASH_ALL,
                                );
                            }
                        }
                        if tx_index == MULTISIG_OUTPUT_LOC.0 && index == MULTISIG_OUTPUT_LOC.1 {
                            multisig_type_hash = output
                                .type_()
                                .to_opt()
                                .map(|script| script.calc_script_hash());
                            let data_hash = CellOutput::calc_data_hash(&data.raw_data());
                            if data_hash != CODE_HASH_SECP256K1_BLAKE160_MULTISIG_ALL.pack() {
                                log::error!(
                                    "System multisig script code hash error! found: {}, expected: {}",
                                    data_hash,
                                    CODE_HASH_SECP256K1_BLAKE160_MULTISIG_ALL,
                                );
                            }
                        }
                        if tx_index == DAO_OUTPUT_LOC.0 && index == DAO_OUTPUT_LOC.1 {
                            dao_type_hash = output
                                .type_()
                                .to_opt()
                                .map(|script| script.calc_script_hash());
                            let data_hash = CellOutput::calc_data_hash(&data.raw_data());
                            if data_hash != CODE_HASH_DAO.pack() {
                                log::error!(
                                    "System dao script code hash error! found: {}, expected: {}",
                                    data_hash,
                                    CODE_HASH_DAO,
                                );
                            }
                        }
                        OutPoint::new(tx.hash(), index as u32)
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();

        let sighash_type_hash = sighash_type_hash
            .ok_or_else(|| "No type hash(sighash) found in txs[0][1]".to_owned())
            .map_err(ParseGenesisInfoError::TypeHashNotFound)?;
        let multisig_type_hash = multisig_type_hash
            .ok_or_else(|| "No type hash(multisig) found in txs[0][4]".to_owned())
            .map_err(ParseGenesisInfoError::TypeHashNotFound)?;
        let dao_type_hash = dao_type_hash
            .ok_or_else(|| "No type hash(dao) found in txs[0][2]".to_owned())
            .map_err(ParseGenesisInfoError::TypeHashNotFound)?;

        let sighash_dep = CellDep::new_builder()
            .out_point(out_points[SIGHASH_GROUP_OUTPUT_LOC.0][SIGHASH_GROUP_OUTPUT_LOC.1].clone())
            .dep_type(DepType::DepGroup.into())
            .build();
        let multisig_dep = CellDep::new_builder()
            .out_point(out_points[MULTISIG_GROUP_OUTPUT_LOC.0][MULTISIG_GROUP_OUTPUT_LOC.1].clone())
            .dep_type(DepType::DepGroup.into())
            .build();
        let dao_dep = CellDep::new_builder()
            .out_point(out_points[DAO_OUTPUT_LOC.0][DAO_OUTPUT_LOC.1].clone())
            .build();

        let mut items = HashMap::default();
        items.insert(
            ScriptId::new_type(sighash_type_hash.unpack()),
            (sighash_dep, "Secp256k1 blake160 sighash all".to_string()),
        );
        items.insert(
            ScriptId::new_type(multisig_type_hash.unpack()),
            (multisig_dep, "Secp256k1 blake160 multisig all".to_string()),
        );
        items.insert(
            ScriptId::new_type(dao_type_hash.unpack()),
            (dao_dep, "Nervos DAO".to_string()),
        );
        let offchain = OffchainCellDepResolver { items };
        Ok(DefaultCellDepResolver { offchain })
    }
    pub fn insert(
        &mut self,
        script_id: ScriptId,
        cell_dep: CellDep,
        name: String,
    ) -> Option<(CellDep, String)> {
        self.offchain.items.insert(script_id, (cell_dep, name))
    }
    pub fn remove(&mut self, script_id: &ScriptId) -> Option<(CellDep, String)> {
        self.offchain.items.remove(script_id)
    }
    pub fn contains(&self, script_id: &ScriptId) -> bool {
        self.offchain.items.contains_key(script_id)
    }
    pub fn get(&self, script_id: &ScriptId) -> Option<&(CellDep, String)> {
        self.offchain.items.get(script_id)
    }
    pub fn sighash_dep(&self) -> Option<&(CellDep, String)> {
        self.get(&ScriptId::new_type(SIGHASH_TYPE_HASH))
    }
    pub fn multisig_dep(&self) -> Option<&(CellDep, String)> {
        self.get(&ScriptId::new_type(MULTISIG_TYPE_HASH))
    }
    pub fn dao_dep(&self) -> Option<&(CellDep, String)> {
        self.get(&ScriptId::new_type(DAO_TYPE_HASH))
    }
}

impl CellDepResolver for DefaultCellDepResolver {
    fn resolve(&self, script: &Script) -> Option<CellDep> {
        self.offchain.resolve(script)
    }
}

/// A signer use secp256k1 raw key, the id is `blake160(pubkey)`.
#[derive(Default, Clone)]
pub struct SecpCkbRawKeySigner {
    keys: HashMap<H160, secp256k1::SecretKey>,
}

impl SecpCkbRawKeySigner {
    pub fn new(keys: HashMap<H160, secp256k1::SecretKey>) -> SecpCkbRawKeySigner {
        SecpCkbRawKeySigner { keys }
    }
    pub fn new_with_secret_keys(keys: Vec<secp256k1::SecretKey>) -> SecpCkbRawKeySigner {
        let mut signer = SecpCkbRawKeySigner::default();
        for key in keys {
            signer.add_secret_key(key);
        }
        signer
    }
    pub fn add_secret_key(&mut self, key: secp256k1::SecretKey) {
        let pubkey = secp256k1::PublicKey::from_secret_key(&SECP256K1, &key);
        let hash160 = H160::from_slice(&blake2b_256(&pubkey.serialize()[..])[0..20])
            .expect("Generate hash(H160) from pubkey failed");
        self.keys.insert(hash160, key);
    }

    /// Create SecpkRawKeySigner from secret keys for ethereum algorithm.
    pub fn new_with_ethereum_secret_keys(keys: Vec<secp256k1::SecretKey>) -> SecpCkbRawKeySigner {
        let mut signer = SecpCkbRawKeySigner::default();
        for key in keys {
            signer.add_ethereum_secret_key(key);
        }
        signer
    }
    /// Add a ethereum secret key
    pub fn add_ethereum_secret_key(&mut self, key: secp256k1::SecretKey) {
        let pubkey = secp256k1::PublicKey::from_secret_key(&SECP256K1, &key);
        let hash160 = keccak160(Pubkey::from(pubkey).as_ref());
        self.keys.insert(hash160, key);
    }
}

impl Signer for SecpCkbRawKeySigner {
    fn match_id(&self, id: &[u8]) -> bool {
        id.len() == 20 && self.keys.contains_key(&H160::from_slice(id).unwrap())
    }

    fn sign(
        &self,
        id: &[u8],
        message: &[u8],
        recoverable: bool,
        _tx: &TransactionView,
    ) -> Result<Bytes, SignerError> {
        if !self.match_id(id) {
            return Err(SignerError::IdNotFound);
        }
        if message.len() != 32 {
            return Err(SignerError::InvalidMessage(format!(
                "expected length: 32, got: {}",
                message.len()
            )));
        }
        let msg = secp256k1::Message::from_slice(message).expect("Convert to message failed");
        let key = self.keys.get(&H160::from_slice(id).unwrap()).unwrap();
        if recoverable {
            let sig = SECP256K1.sign_ecdsa_recoverable(&msg, key);
            Ok(Bytes::from(serialize_signature(&sig).to_vec()))
        } else {
            let sig = SECP256K1.sign_ecdsa(&msg, key);
            Ok(Bytes::from(sig.serialize_compact().to_vec()))
        }
    }
}

impl Drop for SecpCkbRawKeySigner {
    fn drop(&mut self) {
        for (_, mut secret_key) in self.keys.drain() {
            zeroize_privkey(&mut secret_key);
        }
    }
}
#[cfg(test)]
mod anyhow_tests {
    use anyhow::anyhow;
    #[test]
    fn test_parse_genesis_info_error() {
        let error = super::ParseGenesisInfoError::DataHashNotFound("DataHashNotFound".to_string());
        let error = anyhow!(error);
        assert_eq!("data not found: `DataHashNotFound`", error.to_string());
    }
}
