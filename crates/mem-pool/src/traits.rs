use std::time::Duration;

use anyhow::Result;
use async_trait::async_trait;
use gw_types::{
    offchain::{
        CellWithStatus, CollectedCustodianCells, DepositInfo, ErrorTxReceipt, RollupContext,
    },
    packed::{OutPoint, WithdrawalRequest},
};

#[async_trait]
pub trait MemPoolProvider {
    async fn estimate_next_blocktime(&self) -> Result<Duration>;
    async fn collect_deposit_cells(&self) -> Result<Vec<DepositInfo>>;
    async fn query_available_custodians(
        &self,
        withdrawals: Vec<WithdrawalRequest>,
        last_finalized_block_number: u64,
        rollup_context: RollupContext,
    ) -> Result<CollectedCustodianCells>;
    async fn get_cell(&self, out_point: OutPoint) -> Result<Option<CellWithStatus>>;
    async fn query_mergeable_custodians(
        &self,
        collected_custodians: CollectedCustodianCells,
        last_finalized_block_number: u64,
    ) -> Result<CollectedCustodianCells>;
}

#[async_trait]
pub trait MemPoolErrorTxHandler {
    async fn handle_error_receipt(&mut self, receipt: ErrorTxReceipt) -> Result<()>;
}
