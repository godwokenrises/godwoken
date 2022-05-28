use std::{sync::Arc, time::Instant};

use anyhow::{anyhow, bail, Result};
use gw_common::{registry_address::RegistryAddress, state::State, H256};
use gw_mem_pool::pool::MemPool;
use gw_store::mem_pool_state::MemPoolState;
use gw_traits::CodeStore;
use gw_types::{
    core::{AllowedEoaType, ScriptHashType},
    offchain::RollupContext,
    packed::{L2Transaction, RawL2Transaction, Script},
    prelude::{Builder, Entity, Pack, Unpack},
};
use gw_utils::wallet::Wallet;
use tokio::sync::Mutex;

use crate::mem_execute_tx_state::MemExecuteTxStateTree;

use super::{eth_account_creator::EthAccountCreator, eth_sender::PolyjuiceTxEthSender};

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

pub struct EthContext {
    pub account_context: EthAccountContext,
    pub opt_account_creator: Option<EthAccountCreator>,
}

impl EthContext {
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

    pub async fn create_sender_account_if_not_exists(
        &self,
        tx: L2Transaction,
        snap: &MemPoolState,
        mem_pool: Arc<Mutex<MemPool>>,
    ) -> Result<L2Transaction> {
        let sender_id: u32 = tx.raw().from_id().unpack();
        if 0 != sender_id {
            return Ok(tx);
        }

        let account_creator = match self.opt_account_creator {
            Some(ref creator) => creator,
            None => bail!("no account creator"),
        };

        let snap = snap.load();
        let state = snap.state()?;

        let tx_hash = tx.hash().pack();
        let account_id = match self.recover_sender(&state, &tx)? {
            PolyjuiceTxEthSender::Exist { account_id, .. } => account_id,
            PolyjuiceTxEthSender::New {
                registry_address,
                account_script,
            } => {
                let account_script_hash: H256 = account_script.hash().into();

                // Create account
                let create_tx =
                    account_creator.build_batch_create_tx(&state, vec![account_script])?;
                let create_tx_hash = create_tx.hash().pack();

                log::info!(
                    "[tx from zero] create eth account {:x} for tx {:x}",
                    registry_address.address.pack(),
                    tx_hash
                );

                {
                    log::debug!("[tx from zero] acquire mem_pool");
                    let t = Instant::now();
                    let mut mem_pool = mem_pool.lock().await;
                    log::debug!(
                        "[tx from zero] unlock mem_pool {}ms",
                        t.elapsed().as_millis()
                    );

                    mem_pool.push_transaction(create_tx).await?;
                }

                state
                    .get_account_id_by_script_hash(&account_script_hash)?
                    .ok_or_else(|| {
                        anyhow!(
                            "create tx {:x} for {:x} failed",
                            create_tx_hash,
                            registry_address.address.pack()
                        )
                    })?
            }
        };

        let raw_tx = tx.raw().as_builder().from_id(account_id.pack()).build();
        let tx = tx.as_builder().raw(raw_tx).build();

        log::debug!(
            "[tx from zero] change tx {:x} sender to {} hash {:x}",
            tx_hash,
            account_id,
            tx.hash().pack()
        );

        Ok(tx)
    }

    pub fn mock_sender_if_not_exists<S: State + CodeStore>(
        &self,
        tx: L2Transaction,
        state: &mut MemExecuteTxStateTree<S>,
    ) -> Result<L2Transaction> {
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

                state.mock_account(registry_address, account_script)?
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

    pub fn mock_sender_if_not_exists_from_raw_registery<S: State + CodeStore>(
        &self,
        raw_tx: RawL2Transaction,
        opt_registry_address: Option<RegistryAddress>,
        state: &mut MemExecuteTxStateTree<S>,
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
        let account_id = state.mock_account(registry_address, account_script)?;
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
    ) -> Result<PolyjuiceTxEthSender> {
        PolyjuiceTxEthSender::recover(&self.account_context, state, tx)
    }
}
