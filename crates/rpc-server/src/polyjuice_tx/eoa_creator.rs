#![allow(clippy::mutable_key_type)]

use std::collections::HashMap;

use anyhow::{anyhow, bail, Result};
use gw_common::builtins::{CKB_SUDT_ACCOUNT_ID, ETH_REGISTRY_ACCOUNT_ID, RESERVED_ACCOUNT_ID};
use gw_common::state::State;
use gw_common::H256;
use gw_generator::account_lock_manage::secp256k1::Secp256k1Eth;
use gw_traits::CodeStore;
use gw_types::packed::{
    BatchCreateEthAccounts, Fee, L2Transaction, MetaContractArgs, RawL2Transaction, Script,
    ScriptVec,
};
use gw_types::prelude::{Builder, Entity, Pack, Unpack};
use gw_types::{bytes::Bytes, U256};
use gw_utils::wallet::Wallet;
use tracing::instrument;

use super::{recover_registry_address, to_eth_eoa_script};

type Signature = Bytes;
type EthEOAScript = Script;

const META_CONTRACT_ACCOUNT_ID: u32 = RESERVED_ACCOUNT_ID;
pub const MAX_EOA_PER_BATCH: usize = 50;

pub struct PolyjuiceEthEoaCreator {
    chain_id: u64,
    rollup_script_hash: H256,
    meta_contract_script_hash: H256,
    creator_script_hash: H256,
    eth_lock_code_hash: H256,
    creator_wallet: Wallet,
}

impl PolyjuiceEthEoaCreator {
    pub fn create(
        state: &(impl State + CodeStore),
        chain_id: u64,
        rollup_script_hash: H256,
        eth_lock_code_hash: H256,
        creator_wallet: Wallet,
    ) -> Result<Self> {
        // NOTE: Use eth_lock_code_hash to ensure creator tx can be verified on chain
        let creator_script_hash = {
            let script =
                creator_wallet.eth_lock_script(&rollup_script_hash, &eth_lock_code_hash)?;
            script.hash().into()
        };
        let meta_contract_script_hash = state.get_script_hash(META_CONTRACT_ACCOUNT_ID)?;

        let creator = PolyjuiceEthEoaCreator {
            chain_id,
            rollup_script_hash,
            meta_contract_script_hash,
            creator_script_hash,
            eth_lock_code_hash,
            creator_wallet,
        };

        Ok(creator)
    }

    #[instrument(skip_all)]
    pub fn filter_map_from_id_zero_has_ckb_balance<'a>(
        &self,
        state: &(impl State + CodeStore),
        txs: impl IntoIterator<Item = &'a L2Transaction>,
    ) -> HashMap<Signature, EthEOAScript> {
        let check_non_existent_eoa_has_ckb_balance = |tx: &L2Transaction| -> _ {
            let registry_address = recover_registry_address(
                self.chain_id,
                state,
                &tx.raw(),
                &tx.signature().unpack(),
            )?;

            if let Some(hash) = state.get_script_hash_by_registry_address(&registry_address)? {
                bail!(
                    "registry address {:x} is mapped to script hash {:x}",
                    registry_address.address.pack(),
                    hash.pack()
                );
            }

            let balance = state.get_sudt_balance(CKB_SUDT_ACCOUNT_ID, &registry_address)?;
            if U256::zero() == balance {
                bail!(
                    "registry address {:x} doesn't have ckb balance",
                    registry_address.address.pack()
                );
            }

            Ok(registry_address)
        };

        txs.into_iter()
            .filter_map(|tx| match check_non_existent_eoa_has_ckb_balance(tx) {
                Err(err) => {
                    log::debug!("[polyjuice eoa creator] tx {:x} {}", tx.hash().pack(), err);
                    None
                }
                Ok(registry_address) => Some((
                    tx.signature().unpack(),
                    to_eth_eoa_script(
                        self.rollup_script_hash,
                        self.eth_lock_code_hash,
                        &registry_address,
                    ),
                )),
            })
            .collect()
    }

    #[instrument(skip_all)]
    pub fn build_batch_create_tx<'a>(
        &self,
        state: &impl State,
        eoa_scripts: impl IntoIterator<Item = &'a EthEOAScript>,
    ) -> Result<L2Transaction> {
        let eoa_scripts: Vec<_> = eoa_scripts.into_iter().cloned().collect();
        let creator_account_id = state
            .get_account_id_by_script_hash(&self.creator_script_hash)?
            .ok_or_else(|| anyhow!("[polyjuice eoa creator] creator account id not found"))?;

        let creator_registry_address = match state.get_registry_address_by_script_hash(
            ETH_REGISTRY_ACCOUNT_ID,
            &self.creator_script_hash,
        )? {
            Some(addr) => addr,
            None => {
                bail!("[polyjuice eoa creator] creator eth registry address not found")
            }
        };

        let nonce = state.get_nonce(creator_account_id)?;

        let fee = Fee::new_builder()
            .registry_id(ETH_REGISTRY_ACCOUNT_ID.pack())
            .amount(0u128.pack())
            .build();
        let batch_create = BatchCreateEthAccounts::new_builder()
            .fee(fee)
            .scripts(ScriptVec::new_builder().set(eoa_scripts).build())
            .build();
        let args = MetaContractArgs::new_builder().set(batch_create).build();

        let raw_l2tx = RawL2Transaction::new_builder()
            .chain_id(self.chain_id.pack())
            .from_id(creator_account_id.pack())
            .to_id(META_CONTRACT_ACCOUNT_ID.pack())
            .nonce(nonce.pack())
            .args(args.as_bytes().pack())
            .build();

        let signing_message = Secp256k1Eth::eip712_signing_message(
            self.chain_id,
            &raw_l2tx,
            creator_registry_address,
            self.meta_contract_script_hash,
        )?;
        let sign = self.creator_wallet.sign_message(signing_message.into())?;

        let tx = L2Transaction::new_builder()
            .raw(raw_l2tx)
            .signature(sign.pack())
            .build();

        Ok(tx)
    }
}
