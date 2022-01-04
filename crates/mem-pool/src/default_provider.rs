use std::{sync::Arc, time::Duration};

use anyhow::{anyhow, Result};
use gw_poa::PoA;
use gw_rpc_client::rpc_client::RPCClient;
use gw_runtime::spawn;
use gw_store::Store;
use gw_types::{
    offchain::{
        CellWithStatus, CollectedCustodianCells, DepositInfo, InputCellInfo, RollupContext,
    },
    packed::{CellInput, OutPoint, WithdrawalRequest},
    prelude::*,
};
use tokio::{sync::Mutex, task::JoinHandle};

use crate::{
    constants::{MAX_MEM_BLOCK_DEPOSITS, MIN_CKB_DEPOSIT_CAPACITY, MIN_SUDT_DEPOSIT_CAPACITY},
    custodian::{query_finalized_custodians, query_mergeable_custodians},
    traits::MemPoolProvider,
};

pub struct DefaultMemPoolProvider {
    /// RPC client
    rpc_client: RPCClient,
    /// POA Context
    poa: Arc<Mutex<PoA>>,
    store: Store,
}

impl DefaultMemPoolProvider {
    pub fn new(rpc_client: RPCClient, poa: Arc<Mutex<PoA>>, store: Store) -> Self {
        DefaultMemPoolProvider {
            rpc_client,
            poa,
            store,
        }
    }
}

impl MemPoolProvider for DefaultMemPoolProvider {
    fn estimate_next_blocktime(&self) -> JoinHandle<Result<Duration>> {
        // estimate next l2block timestamp
        let poa = Arc::clone(&self.poa);
        let rpc_client = self.rpc_client.clone();
        spawn(async move {
            let poa = poa.lock().await;
            let rollup_cell = rpc_client
                .query_rollup_cell()
                .await?
                .ok_or_else(|| anyhow!("can't find rollup cell"))?;
            let input_cell = InputCellInfo {
                input: CellInput::new_builder()
                    .previous_output(rollup_cell.out_point.clone())
                    .build(),
                cell: rollup_cell,
            };
            let ctx = poa.query_poa_context(&input_cell).await?;
            // TODO how to estimate a more accurate timestamp?
            let timestamp = poa.estimate_next_round_start_time(ctx);
            Ok(timestamp)
        })
    }

    fn collect_deposit_cells(&self) -> JoinHandle<Result<Vec<DepositInfo>>> {
        let rpc_client = self.rpc_client.clone();
        spawn(async move {
            rpc_client
                .query_deposit_cells(
                    MAX_MEM_BLOCK_DEPOSITS,
                    MIN_CKB_DEPOSIT_CAPACITY,
                    MIN_SUDT_DEPOSIT_CAPACITY,
                )
                .await
        })
    }

    fn get_cell(&self, out_point: OutPoint) -> JoinHandle<Result<Option<CellWithStatus>>> {
        let rpc_client = self.rpc_client.clone();
        spawn(async move { rpc_client.get_cell(out_point).await })
    }

    fn query_available_custodians(
        &self,
        withdrawals: Vec<WithdrawalRequest>,
        last_finalized_block_number: u64,
        rollup_context: RollupContext,
    ) -> JoinHandle<Result<CollectedCustodianCells>> {
        let rpc_client = self.rpc_client.clone();
        let db = self.store.begin_transaction();
        spawn(async move {
            let r = query_finalized_custodians(
                &rpc_client,
                &db,
                withdrawals.clone().into_iter(),
                &rollup_context,
                last_finalized_block_number,
            )
            .await?;
            Ok(r.expect_any())
        })
    }

    fn query_mergeable_custodians(
        &self,
        collected_custodians: CollectedCustodianCells,
        last_finalized_block_number: u64,
    ) -> JoinHandle<Result<CollectedCustodianCells>> {
        let rpc_client = self.rpc_client.clone();
        spawn(async move {
            let r = query_mergeable_custodians(
                &rpc_client,
                collected_custodians,
                last_finalized_block_number,
            )
            .await?;
            Ok(r.expect_any())
        })
    }
}
