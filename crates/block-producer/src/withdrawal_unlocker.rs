#![allow(clippy::mutable_key_type)]

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use anyhow::{bail, Result};
use async_trait::async_trait;
use gw_common::H256;
use gw_config::{ContractsCellDep, DebugConfig};
use gw_rpc_client::contract::ContractsCellDepManager;
use gw_rpc_client::rpc_client::RPCClient;
use gw_types::offchain::{global_state_from_slice, CellInfo, RollupContext, TxStatus};
use gw_types::packed::{OutPoint, Transaction};
use gw_types::prelude::{Pack, Unpack};
use gw_utils::fee::fill_tx_fee;
use gw_utils::genesis_info::CKBGenesisInfo;
use gw_utils::transaction_skeleton::TransactionSkeleton;
use gw_utils::wallet::Wallet;
use tracing::instrument;

use crate::types::ChainEvent;
use crate::utils;

pub use gw_rpc_client::contract::Guard;

const TRANSACTION_FAILED_TO_RESOLVE_ERROR: &str = "TransactionFailedToResolve";

pub struct FinalizedWithdrawalUnlocker {
    unlocker: DefaultUnlocker,
    unlocked_set: HashSet<OutPoint>,
    unlock_txs: HashMap<H256, Vec<OutPoint>>,
    debug_config: DebugConfig,
}

impl FinalizedWithdrawalUnlocker {
    pub fn new(
        rpc_client: RPCClient,
        ckb_genesis_info: CKBGenesisInfo,
        contracts_dep_manager: ContractsCellDepManager,
        wallet: Wallet,
        debug_config: DebugConfig,
    ) -> Self {
        let unlocker =
            DefaultUnlocker::new(rpc_client, ckb_genesis_info, contracts_dep_manager, wallet);

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
                    log::info!("[unlock withdrawal] get unlock tx failed {}, drop it", err);
                    drop_txs.push(*tx_hash);
                }
                Ok(None) => {
                    log::info!("[unlock withdrawal] dropped unlock tx {}", tx_hash.pack());
                    drop_txs.push(*tx_hash);
                }
                Ok(Some(tx_status)) => {
                    match tx_status {
                        TxStatus::Pending | TxStatus::Proposed => continue, // Wait
                        TxStatus::Committed => {
                            log::info!(
                                "[unlock withdrawal] unlock {} withdrawals in tx {}",
                                withdrawal_to_unlock.len(),
                                tx_hash.pack(),
                            );
                        }
                        TxStatus::Unknown | TxStatus::Rejected => {
                            log::debug!(
                                "[unlock withdrawal] unlock withdrawals tx {} status {:?}, drop it",
                                tx_hash.pack(),
                                tx_status
                            );
                        }
                        _ => {
                            log::warn!(
                                "[unlock withdrawal] unhandled unlock withdrawals tx {} status {:?}, drop it",
                                tx_hash.pack(),
                                tx_status
                            )
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
    fn rollup_context(&self) -> &RollupContext;

    fn contracts_dep(&self) -> Guard<Arc<ContractsCellDep>>;

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

        let to_unlock = match crate::withdrawal::deprecated::unlock_to_owner(
            rollup_cell,
            self.rollup_context(),
            &self.contracts_dep(),
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
    contracts_dep_manager: ContractsCellDepManager,
    wallet: Wallet,
}

impl DefaultUnlocker {
    pub const MAX_WITHDRAWALS_PER_TX: usize = 100;

    pub fn new(
        rpc_client: RPCClient,
        ckb_genesis_info: CKBGenesisInfo,
        contracts_dep_manager: ContractsCellDepManager,
        wallet: Wallet,
    ) -> Self {
        DefaultUnlocker {
            rpc_client,
            ckb_genesis_info,
            contracts_dep_manager,
            wallet,
        }
    }
}

#[async_trait]
impl BuildUnlockWithdrawalToOwner for DefaultUnlocker {
    fn rollup_context(&self) -> &RollupContext {
        &self.rpc_client.rollup_context
    }

    fn contracts_dep(&self) -> Guard<Arc<ContractsCellDep>> {
        self.contracts_dep_manager.load()
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
