use std::collections::HashMap;

use anyhow::{anyhow, bail, Result};
use gw_common::{builtins::CKB_SUDT_ACCOUNT_ID, registry_address::RegistryAddress, state::State};
use gw_generator::{error::TransactionError, typed_transaction::types::TypedRawTransaction};
use gw_store::state::{traits::JournalDB, MemStateDB};
use gw_traits::CodeStore;
use gw_types::{
    bytes::Bytes,
    core::{AllowedContractType, AllowedEoaType, ScriptHashType},
    h256::*,
    packed::{L2Transaction, RawL2Transaction, Script},
    prelude::{Builder, Entity, Pack, Unpack},
};
use gw_utils::wallet::Wallet;
use gw_utils::RollupContext;
use tracing::instrument;

use crate::mem_execute_tx_state::mock_account;

use super::{
    error::PolyjuiceTxSenderRecoverError, eth_account_creator::EthAccountCreator,
    eth_sender::PolyjuiceTxEthSender,
};

#[derive(Clone)]
pub struct EthAccountContext {
    pub chain_id: u64,
    pub rollup_script_hash: H256,
    pub eth_lock_code_hash: H256,
    pub polyjuice_validator_code_hash: H256,
}

impl EthAccountContext {
    pub fn new(
        chain_id: u64,
        rollup_script_hash: H256,
        eth_lock_code_hash: H256,
        polyjuice_validator_code_hash: H256,
    ) -> Self {
        Self {
            chain_id,
            rollup_script_hash,
            eth_lock_code_hash,
            polyjuice_validator_code_hash,
        }
    }

    pub fn to_account_script(&self, registry_address: &RegistryAddress) -> Script {
        let mut args = self.rollup_script_hash.as_slice().to_vec();
        args.extend_from_slice(&registry_address.address);

        Script::new_builder()
            .code_hash(self.eth_lock_code_hash.pack())
            .hash_type(ScriptHashType::Type.into())
            .args(args.pack())
            .build()
    }
}

pub struct RecoveredSenders {
    sig_senders: HashMap<Bytes, PolyjuiceTxEthSender>,
}

impl RecoveredSenders {
    pub fn build_create_tx(
        &self,
        ctx: &EthRecover,
        state: &(impl State + CodeStore),
    ) -> Result<Option<L2Transaction>> {
        let account_creator = match ctx.opt_account_creator {
            Some(ref creator) => creator,
            None => bail!("no account creator"),
        };

        let filter_new_accounts = self.sig_senders.values().filter_map(|sender| match sender {
            PolyjuiceTxEthSender::New { account_script, .. } => Some(account_script.to_owned()),
            _ => None,
        });

        let new_account_scripts = filter_new_accounts.collect::<Vec<_>>();
        if new_account_scripts.is_empty() {
            return Ok(None);
        }

        log::info!(
            "[tx from zero] create accounts {}",
            new_account_scripts.len()
        );

        let tx = account_creator.build_batch_create_tx(state, new_account_scripts)?;
        Ok(Some(tx))
    }

    pub fn get_account_id(&self, sig: &Bytes, state: &impl State) -> Result<u32> {
        let account_script_hash = match self.sig_senders.get(sig) {
            Some(PolyjuiceTxEthSender::Exist { account_id, .. }) => return Ok(*account_id),
            Some(PolyjuiceTxEthSender::New { account_script, .. }) => account_script.hash(),
            None => bail!("no sender recovered"),
        };

        state
            .get_account_id_by_script_hash(&account_script_hash)?
            .ok_or_else(|| anyhow!("account id not found"))
    }
}

pub struct EthRecover {
    pub account_context: EthAccountContext,
    pub opt_account_creator: Option<EthAccountCreator>,
}

impl EthRecover {
    pub fn create(rollup_context: &RollupContext, creator_wallet: Option<Wallet>) -> Result<Self> {
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
        let polyjuice_validator_code_hash = {
            let allowed_contract_type_hashes =
                rollup_context.rollup_config.allowed_contract_type_hashes();
            allowed_contract_type_hashes
                .as_reader()
                .iter()
                .find_map(|type_hash| {
                    if type_hash.type_().to_entity() == AllowedContractType::Polyjuice.into() {
                        Some(type_hash.hash().unpack())
                    } else {
                        None
                    }
                })
                .ok_or_else(|| anyhow!("polyjuice validator code hash not found"))?
        };

        let account_context = EthAccountContext::new(
            chain_id,
            rollup_script_hash,
            eth_lock_code_hash,
            polyjuice_validator_code_hash,
        );
        let opt_account_creator = creator_wallet
            .map(|wallet| EthAccountCreator::create(&account_context, wallet))
            .transpose()?;

        let ctx = EthRecover {
            account_context,
            opt_account_creator,
        };

        Ok(ctx)
    }

    #[instrument(skip_all)]
    pub fn recover_sender_accounts<'a>(
        &self,
        txs_from_zero: impl Iterator<Item = &'a L2Transaction>,
        state: &(impl State + CodeStore),
    ) -> RecoveredSenders {
        let sig_senders = txs_from_zero.filter_map(|tx| {
            let sender_id: u32 = tx.raw().from_id().unpack();
            if 0 != sender_id {
                return None;
            }

            // Don't create account for insufficient balance
            let recover_and_check_balance = |tx| -> _ {
                let sender = self.recover_sender(state, tx)?;
                let typed_tx =
                    TypedRawTransaction::from_tx(tx.raw(), AllowedContractType::Polyjuice)
                        .ok_or_else(|| anyhow!("unknown tx type"))?;
                let tx_cost = typed_tx.cost().ok_or(TransactionError::NoCost)?;
                let balance =
                    state.get_sudt_balance(CKB_SUDT_ACCOUNT_ID, sender.registry_address())?;

                if balance < tx_cost {
                    bail!(TransactionError::InsufficientBalance)
                }

                Ok(sender)
            };

            match recover_and_check_balance(tx) {
                Ok(sender) => Some((tx.signature().unpack(), sender)),
                Err(err) => {
                    log::info!("[tx from zero] recover {:x} {}", tx.hash().pack(), err);
                    None
                }
            }
        });

        RecoveredSenders {
            sig_senders: sig_senders.collect(),
        }
    }

    pub fn mock_sender_if_not_exists(
        &self,
        tx: L2Transaction,
        state: &mut MemStateDB,
    ) -> Result<L2Transaction, PolyjuiceTxSenderRecoverError> {
        let sender_id: u32 = tx.raw().from_id().unpack();
        if 0 != sender_id {
            return Ok(tx);
        }

        let tx_hash = tx.hash().pack();
        let account_id = match self.recover_sender(state, &tx)? {
            PolyjuiceTxEthSender::Exist { account_id, .. } => account_id,
            PolyjuiceTxEthSender::New {
                account_script,
                registry_address,
            } => {
                log::debug!(
                    "[tx from zero] mock account {:x} for {:x}",
                    registry_address.address.pack(),
                    tx_hash,
                );

                mock_account(state, registry_address, account_script)?
            }
        };

        let raw_tx = tx.raw().as_builder().from_id(account_id.pack()).build();
        let tx = tx.as_builder().raw(raw_tx).build();

        log::debug!(
            "[tx from zero] mock tx {:x} sender to {} hash {:x}",
            tx_hash,
            account_id,
            tx.hash().pack()
        );

        Ok(tx)
    }

    pub fn mock_sender_if_not_exists_from_raw_registry<S: State + CodeStore + JournalDB>(
        &self,
        raw_tx: RawL2Transaction,
        opt_registry_address: Option<RegistryAddress>,
        state: &mut S,
    ) -> Result<RawL2Transaction> {
        let sender_id: u32 = raw_tx.from_id().unpack();
        if 0 != sender_id {
            return Ok(raw_tx);
        }

        let registry_address = match opt_registry_address {
            Some(addr) => addr,
            None => bail!("no registry address"),
        };

        let tx_hash = raw_tx.hash().pack();
        log::debug!(
            "[tx from zero] mock account {:x} for {:x}",
            registry_address.address.pack(),
            tx_hash,
        );

        let account_script = self.account_context.to_account_script(&registry_address);
        let account_id = mock_account(state, registry_address, account_script)?;
        let raw_tx = raw_tx.as_builder().from_id(account_id.pack()).build();

        log::debug!(
            "[tx from zero] mock tx {:x} sender to {} hash {:x}",
            tx_hash,
            account_id,
            raw_tx.hash().pack()
        );

        Ok(raw_tx)
    }

    fn recover_sender(
        &self,
        state: &(impl State + CodeStore),
        tx: &L2Transaction,
    ) -> Result<PolyjuiceTxEthSender, PolyjuiceTxSenderRecoverError> {
        PolyjuiceTxEthSender::recover(&self.account_context, state, tx)
    }
}
