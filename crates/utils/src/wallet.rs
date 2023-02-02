use std::path::Path;

use anyhow::{anyhow, ensure, Context, Result};
use ckb_crypto::secp::Privkey;
use ckb_types::h256;
use faster_hex::hex_decode;
use gw_common::blake2b::{self, new_blake2b};
use gw_config::WalletConfig;
use gw_types::{
    bytes::Bytes,
    core::ScriptHashType,
    h256::*,
    packed::{Script, Transaction},
    prelude::*,
};
use sha3::{Digest, Keccak256};

use crate::transaction_skeleton::{Signature, TransactionSkeleton};

pub const SIGHASH_TYPE_HASH: H256 =
    h256!("0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8").0;

pub struct Wallet {
    privkey: Privkey,
    lock: Script,
}

impl TryFrom<Privkey> for Wallet {
    type Error = ckb_crypto::secp::Error;

    fn try_from(privkey: Privkey) -> Result<Self, Self::Error> {
        let pk = privkey.pubkey()?.serialize();
        let pk160 = &blake2b::hash(&pk)[..20];
        Ok(Self {
            lock: Script::new_builder()
                .code_hash(SIGHASH_TYPE_HASH.pack())
                .hash_type(ScriptHashType::Type.into())
                .args(pk160.pack())
                .build(),
            privkey,
        })
    }
}

impl Wallet {
    pub fn new(privkey: Privkey, lock: Script) -> Self {
        Wallet { privkey, lock }
    }

    pub fn from_privkey_path(p: &Path) -> Result<Self> {
        let privkey = {
            let content = std::fs::read_to_string(p).context("read wallet privkey")?;
            let content = content.trim_start_matches("0x").trim();
            ensure!(content.as_bytes().len() == 64, "invalid privkey length");
            let mut decoded = [0u8; 32];
            hex_decode(content.as_bytes(), &mut decoded)?;
            Privkey::from_slice(&decoded)
        };
        let wallet = Self::try_from(privkey)?;
        Ok(wallet)
    }

    pub fn from_config(config: &WalletConfig) -> Result<Self> {
        Self::from_privkey_path(&config.privkey_path)
    }

    pub fn lock_script(&self) -> &Script {
        &self.lock
    }

    pub fn eth_lock_script(
        &self,
        rollup_script_hash: &H256,
        eth_account_lock_code_hash: &H256,
    ) -> Result<Script> {
        privkey_to_eth_account_script(
            &self.privkey,
            rollup_script_hash,
            eth_account_lock_code_hash,
        )
    }

    // sign message
    pub fn sign_message(&self, msg: [u8; 32]) -> Result<[u8; 65]> {
        let signature = self
            .privkey
            .sign_recoverable(&msg.into())
            .map_err(|err| anyhow!("signing error: {}", err))?;
        let mut inner = [0u8; 65];
        inner.copy_from_slice(&signature.serialize());
        Ok(inner)
    }

    pub fn sign_tx_skeleton(&self, tx_skeleton: TransactionSkeleton) -> Result<Transaction> {
        let signature_entries = tx_skeleton.signature_entries();
        let dummy_signatures: Vec<_> = {
            let entries = signature_entries.iter();
            entries.map(Signature::zero_bytes_from_entry).collect()
        };

        // seal a dummy tx for calculation
        let tx = tx_skeleton
            .seal(&signature_entries, dummy_signatures)?
            .transaction;
        let tx_hash = {
            let mut hasher = new_blake2b();
            hasher.update(tx.raw().as_slice());
            let mut hash = [0u8; 32];
            hasher.finalize(&mut hash);
            hash
        };
        let mut signatures = Vec::with_capacity(signature_entries.len());
        for entry in &signature_entries {
            let mut hasher = new_blake2b();
            // hash tx_hash
            hasher.update(&tx_hash);
            // hash the first witness: len | witness
            let first_witness: Bytes = tx
                .witnesses()
                .get(entry.indexes[0])
                .expect("get first witness")
                .unpack();
            hasher.update(&(first_witness.len() as u64).to_le_bytes());
            hasher.update(&first_witness);
            // hash the other witnesses in the group
            for &index in &entry.indexes[1..] {
                let witness: Bytes = tx.witnesses().get(index).expect("get witness").unpack();
                hasher.update(&(witness.len() as u64).to_le_bytes());
                hasher.update(&witness);
            }
            // hash witnesses which do not in any input group
            for index in tx.raw().inputs().len()..tx.witnesses().len() {
                let witness: Bytes = tx.witnesses().get(index).expect("get witness").unpack();
                hasher.update(&(witness.len() as u64).to_le_bytes());
                hasher.update(&witness);
            }
            let mut message = [0u8; 32];
            hasher.finalize(&mut message);
            // sign tx
            let signature = Signature::new(entry.kind, self.sign_message(message)?);
            signatures.push(signature.as_bytes());
        }
        // seal
        let sealed_tx = tx_skeleton.seal(&signature_entries, signatures)?;
        // check fee rate
        sealed_tx.check_fee_rate()?;
        Ok(sealed_tx.transaction)
    }
}

pub fn privkey_to_eth_account_script(
    privkey: &Privkey,
    rollup_script_hash: &H256,
    eth_account_lock_code_hash: &H256,
) -> Result<Script> {
    let pubkey = secp256k1::PublicKey::from_slice(&privkey.pubkey()?.serialize())?;
    let pubkey_hash = {
        let mut hasher = Keccak256::new();
        hasher.update(&pubkey.serialize_uncompressed()[1..]);
        hasher.finalize()
    };

    let mut args = Vec::with_capacity(32 + 20);
    args.extend(rollup_script_hash.as_slice());
    args.extend(&pubkey_hash[12..]);

    let script = Script::new_builder()
        .code_hash(eth_account_lock_code_hash.pack())
        .hash_type(ScriptHashType::Type.into())
        .args(args[..].pack())
        .build();

    Ok(script)
}
