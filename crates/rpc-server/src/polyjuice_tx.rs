use anyhow::{anyhow, Result};
use gw_common::{state::State, H256};
use gw_generator::Generator;
use gw_traits::CodeStore;
use gw_types::{core::AllowedEoaType, packed::L2Transaction, prelude::Unpack};
use gw_utils::wallet::Wallet;

use self::{eth_account_creator::EthAccountCreator, eth_sender::PolyjuiceTxEthSender};

pub mod eth_account_creator;
pub mod eth_sender;

#[derive(Clone)]
pub struct EthAccountContext {
    pub chain_id: u64,
    pub rollup_script_hash: H256,
    pub eth_lock_code_hash: H256,
}

impl EthAccountContext {
    pub fn new(chain_id: u64, rollup_script_hash: H256, eth_lock_code_hash: H256) -> Self {
        Self {
            chain_id,
            rollup_script_hash,
            eth_lock_code_hash,
        }
    }
}

pub struct EthContext {
    pub account_context: EthAccountContext,
    pub opt_account_creator: Option<EthAccountCreator>,
}

impl EthContext {
    pub fn create(generator: &Generator, creator_wallet: Option<Wallet>) -> Result<Self> {
        let rollup_context = generator.rollup_context();
        let chain_id = rollup_context.rollup_config.chain_id().unpack();
        let rollup_script_hash = rollup_context.rollup_script_hash;
        let eth_lock_code_hash = {
            let allowed_eoa_type_hashes = rollup_context.rollup_config.allowed_eoa_type_hashes();
            allowed_eoa_type_hashes
                .as_reader()
                .iter()
                .find_map(|type_hash| {
                    if type_hash.type_().to_entity() == AllowedEoaType::Eth.into() {
                        Some(type_hash.hash().unpack())
                    } else {
                        None
                    }
                })
                .ok_or_else(|| anyhow!("eth lock code hash not found"))?
        };

        let account_context =
            EthAccountContext::new(chain_id, rollup_script_hash, eth_lock_code_hash);
        let opt_account_creator = creator_wallet
            .map(|wallet| EthAccountCreator::create(&account_context, wallet))
            .transpose()?;

        let ctx = EthContext {
            account_context,
            opt_account_creator,
        };

        Ok(ctx)
    }

    pub fn recover_sender(
        &self,
        state: &(impl State + CodeStore),
        tx: &L2Transaction,
    ) -> Result<PolyjuiceTxEthSender> {
        PolyjuiceTxEthSender::recover(&self.account_context, state, tx)
    }
}

pub struct PolyjuiceTxContext {
    pub eth: EthContext,
}

impl PolyjuiceTxContext {
    pub fn new(eth: EthContext) -> Self {
        Self { eth }
    }
}
