#![allow(clippy::mutable_key_type)]

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use anyhow::{bail, Result};
use async_trait::async_trait;
use gw_config::{ContractsCellDep, DebugConfig};
pub use gw_rpc_client::contract::Guard;
use gw_rpc_client::{contract::ContractsCellDepManager, rpc_client::RPCClient};
use gw_types::{
    h256::*,
    offchain::{global_state_from_slice, CellInfo, CompatibleFinalizedTimepoint},
    packed::{OutPoint, RollupConfig, Transaction},
    prelude::*,
};
use gw_utils::{
    fee::fill_tx_fee, genesis_info::CKBGenesisInfo, local_cells::LocalCellsManager,
    query_rollup_cell, transaction_skeleton::TransactionSkeleton, wallet::Wallet,
};
use tokio::sync::Mutex;
use tracing::instrument;

use crate::{types::ChainEvent, utils, utils::global_state_last_finalized_timepoint_to_since};

pub struct FinalizedWithdrawalUnlocker {
    unlocker: DefaultUnlocker,
    unlocked_set: HashSet<OutPoint>,
    unlock_txs: HashMap<H256, Vec<OutPoint>>,
    debug_config: DebugConfig,
}

impl FinalizedWithdrawalUnlocker {
    pub fn new(
        rpc_client: RPCClient,
        local_cells_manager: Arc<Mutex<LocalCellsManager>>,
        ckb_genesis_info: CKBGenesisInfo,
        contracts_dep_manager: ContractsCellDepManager,
        wallet: Wallet,
        debug_config: DebugConfig,
        fee_rate: u64,
    ) -> Self {
        let unlocker = DefaultUnlocker::new(
            rpc_client,
            local_cells_manager,
            ckb_genesis_info,
            contracts_dep_manager,
            wallet,
            fee_rate,
        );

        FinalizedWithdrawalUnlocker {
            unlocker,
            unlocked_set: Default::default(),
            unlock_txs: Default::default(),
            debug_config,
        }
    }

    #[instrument(skip_all, name = "withdrawal unlocker handle_event")]
    pub async fn handle_event(&mut self, _event: &ChainEvent) -> Result<()> {
        let unlocked = &self.unlocked_set;
        let rpc_client = &self.unlocker.rpc_client;
        if let Some((tx, to_unlock)) = self.unlocker.query_and_unlock_to_owner(unlocked).await? {
            let tx_hash = match rpc_client.send_transaction(&tx).await {
                Ok(tx_hash) => tx_hash,
                Err(err) => {
                    let debug_tx_dump_path = &self.debug_config.debug_tx_dump_path;
                    utils::dump_transaction(debug_tx_dump_path, rpc_client, &tx).await;
                    bail!(err);
                }
            };

            log::info!(
                "[unlock withdrawal] try unlock {} withdrawals in tx {}",
                to_unlock.len(),
                tx_hash.pack()
            );

            self.unlocked_set.extend(to_unlock.clone());
            self.unlock_txs.insert(tx_hash, to_unlock);
        }

        // Check unlock tx
        let mut drop_txs = vec![];
        for (tx_hash, withdrawal_to_unlock) in self.unlock_txs.iter() {
            match rpc_client.ckb.get_transaction_status(*tx_hash).await {
                Err(err) => {
                    // Always drop this unlock tx and retry to avoid "lock" withdrawal cell
                    log::info!(
                        "[unlock withdrawal] get unlock tx failed {:#}, drop it",
                        err
                    );
                    drop_txs.push(*tx_hash);
                }
                Ok(None) => {
                    log::info!("[unlock withdrawal] dropped unlock tx {}", tx_hash.pack());
                    drop_txs.push(*tx_hash);
                }
                Ok(Some(tx_status)) => {
                    use gw_jsonrpc_types::ckb_jsonrpc_types::Status;
                    match tx_status {
                        Status::Pending | Status::Proposed => continue, // Wait
                        Status::Committed => {
                            log::info!(
                                "[unlock withdrawal] unlock {} withdrawals in tx {}",
                                withdrawal_to_unlock.len(),
                                tx_hash.pack(),
                            );
                        }
                        Status::Unknown | Status::Rejected => {
                            log::debug!(
                                "[unlock withdrawal] unlock withdrawals tx {} status {:?}, drop it",
                                tx_hash.pack(),
                                tx_status
                            );
                        }
                    }
                    drop_txs.push(*tx_hash);
                }
            }
        }

        for tx_hash in drop_txs {
            if let Some(out_points) = self.unlock_txs.remove(&tx_hash) {
                for out_point in out_points {
                    self.unlocked_set.remove(&out_point);
                }
            }
        }

        Ok(())
    }
}

#[async_trait]
pub trait BuildUnlockWithdrawalToOwner {
    fn rollup_config(&self) -> &RollupConfig;

    fn contracts_dep(&self) -> Guard<Arc<ContractsCellDep>>;

    async fn query_rollup_cell(&self) -> Result<Option<CellInfo>>;

    async fn query_unlockable_withdrawals(
        &self,
        compatible_finalized_timepoint: &CompatibleFinalizedTimepoint,
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
        let compatible_finalized_timepoint = CompatibleFinalizedTimepoint::from_global_state(
            &global_state,
            self.rollup_config().finality_blocks().unpack(),
        );
        let unlockable_withdrawals = self
            .query_unlockable_withdrawals(&compatible_finalized_timepoint, unlocked)
            .await?;
        log::info!(
            "[unlock withdrawal] find unlockable finalized withdrawals {}",
            unlockable_withdrawals.len()
        );

        let global_state_since = global_state_last_finalized_timepoint_to_since(&global_state);
        let to_unlock = match crate::withdrawal::unlock_to_owner(
            rollup_cell,
            self.rollup_config(),
            &self.contracts_dep(),
            unlockable_withdrawals,
            global_state_since,
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
    local_cells_manager: Arc<Mutex<LocalCellsManager>>,
    ckb_genesis_info: CKBGenesisInfo,
    contracts_dep_manager: ContractsCellDepManager,
    wallet: Wallet,
    fee_rate: u64,
}

impl DefaultUnlocker {
    pub const MAX_WITHDRAWALS_PER_TX: usize = 100;

    pub fn new(
        rpc_client: RPCClient,
        local_cells_manager: Arc<Mutex<LocalCellsManager>>,
        ckb_genesis_info: CKBGenesisInfo,
        contracts_dep_manager: ContractsCellDepManager,
        wallet: Wallet,
        fee_rate: u64,
    ) -> Self {
        DefaultUnlocker {
            rpc_client,
            local_cells_manager,
            ckb_genesis_info,
            contracts_dep_manager,
            wallet,
            fee_rate,
        }
    }
}

#[async_trait]
impl BuildUnlockWithdrawalToOwner for DefaultUnlocker {
    fn rollup_config(&self) -> &RollupConfig {
        &self.rpc_client.rollup_config
    }

    fn contracts_dep(&self) -> Guard<Arc<ContractsCellDep>> {
        self.contracts_dep_manager.load()
    }

    async fn query_rollup_cell(&self) -> Result<Option<CellInfo>> {
        let local_cells_manager = self.local_cells_manager.lock().await;
        query_rollup_cell(&local_cells_manager, &self.rpc_client).await
    }

    async fn query_unlockable_withdrawals(
        &self,
        compatible_finalized_timepoint: &CompatibleFinalizedTimepoint,
        unlocked: &HashSet<OutPoint>,
    ) -> Result<Vec<CellInfo>> {
        self.rpc_client
            .query_finalized_owner_lock_withdrawal_cells(
                compatible_finalized_timepoint,
                unlocked,
                Self::MAX_WITHDRAWALS_PER_TX,
            )
            .await
    }

    async fn complete_tx(&self, mut tx_skeleton: TransactionSkeleton) -> Result<Transaction> {
        let owner_lock_dep = self.ckb_genesis_info.sighash_dep();
        tx_skeleton.cell_deps_mut().push(owner_lock_dep);

        let owner_lock = self.wallet.lock_script().to_owned();
        fill_tx_fee(
            &mut tx_skeleton,
            &self.rpc_client.indexer,
            owner_lock,
            self.fee_rate,
        )
        .await?;
        self.wallet.sign_tx_skeleton(tx_skeleton)
    }
}
