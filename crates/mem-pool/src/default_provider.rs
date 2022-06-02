use std::time::Duration;

use anyhow::{bail, Result};
use async_trait::async_trait;
use gw_config::MemBlockConfig;
use gw_rpc_client::rpc_client::RPCClient;
use gw_store::{traits::chain_store::ChainStore, Store};
use gw_types::{
    offchain::{CellWithStatus, CollectedCustodianCells, DepositInfo, RollupContext},
    packed::{OutPoint, WithdrawalRequest},
    prelude::*,
};
use tracing::instrument;

use crate::{
    constants::{MIN_CKB_DEPOSIT_CAPACITY, MIN_SUDT_DEPOSIT_CAPACITY},
    custodian::query_finalized_custodians,
    traits::MemPoolProvider,
};

pub struct DefaultMemPoolProvider {
    /// RPC client
    rpc_client: RPCClient,
    store: Store,
    mem_block_config: MemBlockConfig,
}

impl DefaultMemPoolProvider {
    pub fn new(rpc_client: RPCClient, store: Store, mem_block_config: MemBlockConfig) -> Self {
        DefaultMemPoolProvider {
            rpc_client,
            store,
            mem_block_config,
        }
    }
}

#[async_trait]
impl MemPoolProvider for DefaultMemPoolProvider {
    // estimate next l2block timestamp
    #[instrument(skip_all)]
    async fn estimate_next_blocktime(&self) -> Result<Duration> {
        // Minus one second for first empty block
        const ONE_SECOND: Duration = Duration::from_secs(1);

        let rpc_client = &self.rpc_client;
        let tip_l1_block_hash_number = rpc_client.get_tip().await?;
        let tip_l1_block_hash = tip_l1_block_hash_number.block_hash().unpack();
        if let Some(median_time) = rpc_client.get_block_median_time(tip_l1_block_hash).await? {
            return Ok(median_time - ONE_SECOND);
        }

        // tip l1 block hash is not on the current canonical chain, try parent block hash
        // NOTE: Header doesn't include block hash
        let mut l1_block_number = tip_l1_block_hash_number.number().unpack() + 1;
        loop {
            l1_block_number = l1_block_number.saturating_sub(1);
            let parent_block_hash = match rpc_client.get_header_by_number(l1_block_number).await? {
                Some(header) => header.inner.parent_hash.0.into(),
                None => continue,
            };
            match rpc_client.get_block_median_time(parent_block_hash).await? {
                Some(median_time) => {
                    let median_time = median_time - ONE_SECOND;
                    let tip_block_timestamp = {
                        let block = self.store.get_last_valid_tip_block()?;
                        Duration::from_millis(block.raw().timestamp().unpack())
                    };
                    if median_time <= tip_block_timestamp {
                        bail!("no valid block median time for next block");
                    }
                    return Ok(median_time);
                }
                None => continue,
            }
        }
    }

    #[instrument(skip_all)]
    async fn collect_deposit_cells(&self) -> Result<Vec<DepositInfo>> {
        let rpc_client = self.rpc_client.clone();
        rpc_client
            .query_deposit_cells(
                self.mem_block_config.max_deposits,
                MIN_CKB_DEPOSIT_CAPACITY,
                MIN_SUDT_DEPOSIT_CAPACITY,
            )
            .await
    }

    #[instrument(skip_all)]
    async fn get_cell(&self, out_point: OutPoint) -> Result<Option<CellWithStatus>> {
        self.rpc_client.get_cell(out_point).await
    }

    #[instrument(skip_all)]
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
}
