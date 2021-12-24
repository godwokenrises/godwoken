#![allow(clippy::mutable_key_type)]

use std::collections::HashSet;

use anyhow::{bail, Result};
use async_trait::async_trait;
use gw_config::{BlockProducerConfig, DebugConfig};
use gw_rpc_client::rpc_client::RPCClient;
use gw_types::offchain::{global_state_from_slice, CellInfo, RollupContext};
use gw_types::packed::{OutPoint, Transaction};
use gw_types::prelude::{Pack, Unpack};
use gw_utils::fee::fill_tx_fee;
use gw_utils::genesis_info::CKBGenesisInfo;
use gw_utils::transaction_skeleton::TransactionSkeleton;
use gw_utils::wallet::Wallet;

use crate::types::ChainEvent;
use crate::utils;

const TRANSACTION_FAILED_TO_RESOLVE_ERROR: &str = "TransactionFailedToResolve";

pub struct FinalizedWithdrawalUnlocker {
    unlocker: DefaultUnlocker,
    unlocked_set: HashSet<OutPoint>,
    debug_config: DebugConfig,
}

impl FinalizedWithdrawalUnlocker {
    pub fn new(
        rpc_client: RPCClient,
        ckb_genesis_info: CKBGenesisInfo,
        block_producer_config: BlockProducerConfig,
        wallet: Wallet,
        debug_config: DebugConfig,
    ) -> Self {
        let unlocker =
            DefaultUnlocker::new(rpc_client, ckb_genesis_info, block_producer_config, wallet);

        FinalizedWithdrawalUnlocker {
            unlocker,
            unlocked_set: Default::default(),
            debug_config,
        }
    }

    pub async fn handle_event(&mut self, _event: &ChainEvent) -> Result<()> {
        let unlocked = &self.unlocked_set;
        let rpc_client = &self.unlocker.rpc_client;
        if let Some((tx, to_unlock)) = self.unlocker.query_and_unlock_to_owner(unlocked).await? {
            if let Err(err) = rpc_client.dry_run_transaction(&tx).await {
                let err_string = err.to_string();
                if err_string.contains(TRANSACTION_FAILED_TO_RESOLVE_ERROR) {
                    // NOTE: Maybe unlocked withdrawals are included, this happens after restart.
                    // Wait indexer remove these cells.
                    log::info!(
                        "[unlock withdrawal] failed to resolve, wait unlocked become committed"
                    );
                    return Ok(());
                }
                bail!("dry unlock tx failed {}", err);
            }

            let tx_hash = match rpc_client.send_transaction(&tx).await {
                Ok(tx_hash) => tx_hash,
                Err(err) => {
                    let debug_tx_dump_path = &self.debug_config.debug_tx_dump_path;
                    utils::dump_transaction(debug_tx_dump_path, rpc_client, &tx).await;
                    bail!("send tx failed {}", err);
                }
            };

            // Check unlock tx
            match rpc_client.get_transaction(tx_hash).await {
                Err(err) => bail!("get unlock tx failed {}", err), // Let indexer remove cells for us
                Ok(None) => bail!("unlock tx {} not found", tx_hash.pack()),
                Ok(Some(_)) => (), // Pass
            }

            log::info!(
                "[unlock withdrawal] unlock {} withdrawals in tx {}",
                to_unlock.len(),
                tx_hash.pack()
            );

            self.unlocked_set.extend(to_unlock.clone());
        }

        Ok(())
    }
}

#[async_trait]
pub trait BuildUnlockWithdrawalToOwner {
    fn rollup_context(&self) -> &RollupContext;

    fn block_producer_config(&self) -> &BlockProducerConfig;

    async fn query_rollup_cell(&self) -> Result<Option<CellInfo>>;

    async fn query_unlockable_withdrawals(
        &self,
        last_finalized_block_number: u64,
        unlocked: &HashSet<OutPoint>,
    ) -> Result<Vec<CellInfo>>;

    async fn complete_tx(&self, tx_skeleton: TransactionSkeleton) -> Result<Transaction>;

    async fn query_and_unlock_to_owner(
        &self,
        unlocked: &HashSet<OutPoint>,
    ) -> Result<Option<(Transaction, Vec<OutPoint>)>> {
        let rollup_cell = match self.query_rollup_cell().await? {
            Some(cell) => cell,
            None => {
                log::warn!("rollup cell not found");
                return Ok(None);
            }
        };

        let global_state = global_state_from_slice(&rollup_cell.data)?;
        let last_finalized_block_number: u64 = global_state.last_finalized_block_number().unpack();
        let unlockable_withdrawals = self
            .query_unlockable_withdrawals(last_finalized_block_number, unlocked)
            .await?;
        log::info!(
            "[unlock withdrawal] find unlockable finalized withdrawals {}",
            unlockable_withdrawals.len()
        );

        let to_unlock = match crate::withdrawal::unlock_to_owner(
            rollup_cell,
            self.rollup_context(),
            self.block_producer_config(),
            unlockable_withdrawals,
        )? {
            Some(to_unlock) => to_unlock,
            None => return Ok(None),
        };
        let to_unlock_out_point = {
            let inputs = to_unlock.inputs.iter();
            inputs.map(|i| i.cell.out_point.clone()).collect::<Vec<_>>()
        };

        let mut tx_skeleton = TransactionSkeleton::default();
        tx_skeleton.cell_deps_mut().extend(to_unlock.deps);
        tx_skeleton.inputs_mut().extend(to_unlock.inputs);
        tx_skeleton.witnesses_mut().extend(to_unlock.witness_args);
        tx_skeleton.outputs_mut().extend(to_unlock.outputs);

        let tx = self.complete_tx(tx_skeleton).await?;
        Ok(Some((tx, to_unlock_out_point)))
    }
}

struct DefaultUnlocker {
    rpc_client: RPCClient,
    ckb_genesis_info: CKBGenesisInfo,
    block_producer_config: BlockProducerConfig,
    wallet: Wallet,
}

impl DefaultUnlocker {
    pub const MAX_WITHDRAWALS_PER_TX: usize = 100;

    pub fn new(
        rpc_client: RPCClient,
        ckb_genesis_info: CKBGenesisInfo,
        block_producer_config: BlockProducerConfig,
        wallet: Wallet,
    ) -> Self {
        DefaultUnlocker {
            rpc_client,
            ckb_genesis_info,
            block_producer_config,
            wallet,
        }
    }
}

#[async_trait]
impl BuildUnlockWithdrawalToOwner for DefaultUnlocker {
    fn rollup_context(&self) -> &RollupContext {
        &self.rpc_client.rollup_context
    }

    fn block_producer_config(&self) -> &BlockProducerConfig {
        &self.block_producer_config
    }

    async fn query_rollup_cell(&self) -> Result<Option<CellInfo>> {
        self.rpc_client.query_rollup_cell().await
    }

    async fn query_unlockable_withdrawals(
        &self,
        last_finalized_block_number: u64,
        unlocked: &HashSet<OutPoint>,
    ) -> Result<Vec<CellInfo>> {
        self.rpc_client
            .query_finalized_owner_lock_withdrawal_cells(
                last_finalized_block_number,
                unlocked,
                Self::MAX_WITHDRAWALS_PER_TX,
            )
            .await
    }

    async fn complete_tx(&self, mut tx_skeleton: TransactionSkeleton) -> Result<Transaction> {
        let owner_lock_dep = self.ckb_genesis_info.sighash_dep();
        tx_skeleton.cell_deps_mut().push(owner_lock_dep);

        let owner_lock = self.wallet.lock_script().to_owned();
        fill_tx_fee(&mut tx_skeleton, &self.rpc_client.indexer, owner_lock).await?;
        self.wallet.sign_tx_skeleton(tx_skeleton)
    }
}
