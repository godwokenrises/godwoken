use std::time::Duration;

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use gw_rpc_client::rpc_client::RPCClient;
use gw_store::Store;
use gw_types::{
    offchain::{CellWithStatus, CollectedCustodianCells, DepositInfo, RollupContext},
    packed::{OutPoint, WithdrawalRequest},
    prelude::*,
};

use crate::{
    constants::{MAX_MEM_BLOCK_DEPOSITS, MIN_CKB_DEPOSIT_CAPACITY, MIN_SUDT_DEPOSIT_CAPACITY},
    custodian::{query_finalized_custodians, query_mergeable_custodians},
    traits::MemPoolProvider,
};

pub struct DefaultMemPoolProvider {
    /// RPC client
    rpc_client: RPCClient,
    store: Store,
}

impl DefaultMemPoolProvider {
    pub fn new(rpc_client: RPCClient, store: Store) -> Self {
        DefaultMemPoolProvider { rpc_client, store }
    }
}

#[async_trait]
impl MemPoolProvider for DefaultMemPoolProvider {
    async fn estimate_next_blocktime(&self) -> Result<Duration> {
        // estimate next l2block timestamp
        const ONE_SECOND: Duration = Duration::from_secs(1);
        let rpc_client = &self.rpc_client;
        let tip_block_hash = rpc_client.get_tip().await?.block_hash().unpack();
        let opt_time = rpc_client.get_block_median_time(tip_block_hash).await?;
        // Minus one second for first empty block
        let minus_one_second = opt_time.map(|d| d - ONE_SECOND);
        minus_one_second.ok_or_else(|| anyhow!("tip block median time not found"))
    }

    async fn collect_deposit_cells(&self) -> Result<Vec<DepositInfo>> {
        let rpc_client = self.rpc_client.clone();
        rpc_client
            .query_deposit_cells(
                MAX_MEM_BLOCK_DEPOSITS,
                MIN_CKB_DEPOSIT_CAPACITY,
                MIN_SUDT_DEPOSIT_CAPACITY,
            )
            .await
    }

    async fn get_cell(&self, out_point: OutPoint) -> Result<Option<CellWithStatus>> {
        self.rpc_client.get_cell(out_point).await
    }

    async fn query_available_custodians(
        &self,
        withdrawals: Vec<WithdrawalRequest>,
        last_finalized_block_number: u64,
        rollup_context: RollupContext,
    ) -> Result<CollectedCustodianCells> {
        let db = self.store.begin_transaction();
        let r = query_finalized_custodians(
            &self.rpc_client,
            &db,
            withdrawals.clone().into_iter(),
            &rollup_context,
            last_finalized_block_number,
        )
        .await?;
        Ok(r.expect_any())
    }

    async fn query_mergeable_custodians(
        &self,
        collected_custodians: CollectedCustodianCells,
        last_finalized_block_number: u64,
    ) -> Result<CollectedCustodianCells> {
        let r = query_mergeable_custodians(
            &self.rpc_client,
            collected_custodians,
            last_finalized_block_number,
        )
        .await?;
        Ok(r.expect_any())
    }
}
