use anyhow::Result;
use async_trait::async_trait;
use gw_config::BlockProducerConfig;
use gw_rpc_client::rpc_client::RPCClient;
use gw_types::offchain::{global_state_from_slice, CellInfo, RollupContext};
use gw_types::packed::Transaction;
use gw_types::prelude::{Pack, Unpack};
use gw_utils::fee::fill_tx_fee;
use gw_utils::genesis_info::CKBGenesisInfo;
use gw_utils::transaction_skeleton::TransactionSkeleton;
use gw_utils::wallet::Wallet;

use crate::types::ChainEvent;

// TODO: split into two components
// 1. build unlocked tx and use withdrawal lock script binary to check
// 2. unlocker need to track submit tx and related withdrawal cells, we don't want to re-generate
//    duplicate unlocked cell
pub struct FinalizedWithdrawalUnlocker {
    unlocker: DefaultUnlocker,
}

impl FinalizedWithdrawalUnlocker {
    pub fn new(
        rpc_client: RPCClient,
        ckb_genesis_info: CKBGenesisInfo,
        block_producer_config: BlockProducerConfig,
        wallet: Wallet,
    ) -> Self {
        let unlocker =
            DefaultUnlocker::new(rpc_client, ckb_genesis_info, block_producer_config, wallet);

        FinalizedWithdrawalUnlocker { unlocker }
    }

    pub async fn handle_event(&self, _event: &ChainEvent) -> Result<()> {
        if let Some(tx) = self.unlocker.query_and_unlock_to_owner().await? {
            let tx_hash = self.unlocker.rpc_client.send_transaction(tx).await?;
            log::info!("unlock finalized withdrawal in tx {}", tx_hash.pack());
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
    ) -> Result<Vec<CellInfo>>;

    async fn complete_tx(&self, tx_skeleton: TransactionSkeleton) -> Result<Transaction>;

    async fn query_and_unlock_to_owner(&self) -> Result<Option<Transaction>> {
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
            .query_unlockable_withdrawals(last_finalized_block_number)
            .await?;
        log::info!(
            "find unlockable finalized withdrawals {}",
            unlockable_withdrawals.len()
        );

        let unlocked = match crate::withdrawal::unlock_to_owner(
            rollup_cell,
            self.rollup_context(),
            self.block_producer_config(),
            unlockable_withdrawals,
        )? {
            Some(unlocked) => unlocked,
            None => return Ok(None),
        };

        let mut tx_skeleton = TransactionSkeleton::default();
        tx_skeleton.cell_deps_mut().extend(unlocked.deps);
        tx_skeleton.inputs_mut().extend(unlocked.inputs);
        tx_skeleton.witnesses_mut().extend(unlocked.witness_args);
        tx_skeleton.outputs_mut().extend(unlocked.outputs);

        self.complete_tx(tx_skeleton).await.map(Some)
    }
}

struct DefaultUnlocker {
    rpc_client: RPCClient,
    ckb_genesis_info: CKBGenesisInfo,
    block_producer_config: BlockProducerConfig,
    wallet: Wallet,
}

impl DefaultUnlocker {
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
    ) -> Result<Vec<CellInfo>> {
        self.rpc_client
            .query_finalized_owner_lock_withdrawal_cells(last_finalized_block_number)
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
