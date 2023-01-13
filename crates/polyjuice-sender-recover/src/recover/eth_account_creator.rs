#![allow(clippy::mutable_key_type)]

use anyhow::{anyhow, bail, Result};
use gw_common::builtins::{ETH_REGISTRY_ACCOUNT_ID, RESERVED_ACCOUNT_ID};
use gw_common::state::State;
use gw_generator::account_lock_manage::secp256k1::Secp256k1Eth;
use gw_types::h256::*;
use gw_types::packed::{
    BatchCreateEthAccounts, Fee, L2Transaction, MetaContractArgs, RawL2Transaction, ScriptVec,
};
use gw_types::prelude::*;
use gw_utils::wallet::Wallet;
use tracing::instrument;

use super::eth_recover::EthAccountContext;
use super::eth_sender::EthEOAScript;

const META_CONTRACT_ACCOUNT_ID: u32 = RESERVED_ACCOUNT_ID;
pub const MAX_EOA_PER_BATCH: usize = 50;

pub struct EthAccountCreator {
    chain_id: u64,
    creator_script_hash: H256,
    creator_wallet: Wallet,
}

impl EthAccountCreator {
    pub fn create(ctx: &EthAccountContext, creator_wallet: Wallet) -> Result<Self> {
        // NOTE: Use eth_lock_code_hash to ensure creator tx can be verified on chain
        let creator_script_hash = {
            let script =
                creator_wallet.eth_lock_script(&ctx.rollup_script_hash, &ctx.eth_lock_code_hash)?;
            script.hash()
        };

        let creator = EthAccountCreator {
            chain_id: ctx.chain_id,
            creator_script_hash,
            creator_wallet,
        };

        Ok(creator)
    }

    #[instrument(skip_all)]
    pub fn build_batch_create_tx(
        &self,
        state: &impl State,
        eoa_scripts: Vec<EthEOAScript>,
    ) -> Result<L2Transaction> {
        let creator_account_id = state
            .get_account_id_by_script_hash(&self.creator_script_hash)?
            .ok_or_else(|| anyhow!("[tx from zero] creator account id not found"))?;

        let creator_registry_address = match state.get_registry_address_by_script_hash(
            ETH_REGISTRY_ACCOUNT_ID,
            &self.creator_script_hash,
        )? {
            Some(addr) => addr,
            None => {
                bail!("[tx from zero] creator eth registry address not found")
            }
        };

        let nonce = state.get_nonce(creator_account_id)?;
        let meta_contract_script_hash = state.get_script_hash(META_CONTRACT_ACCOUNT_ID)?;

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
            meta_contract_script_hash,
        )?;
        let sign = self.creator_wallet.sign_message(signing_message)?;

        let tx = L2Transaction::new_builder()
            .raw(raw_l2tx)
            .signature(sign.pack())
            .build();

        Ok(tx)
    }
}
