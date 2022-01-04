use std::time::Duration;

use anyhow::Result;
use gw_types::{
    offchain::{
        CellWithStatus, CollectedCustodianCells, DepositInfo, ErrorTxReceipt, RollupContext,
    },
    packed::{OutPoint, WithdrawalRequest},
};
use tokio::task::JoinHandle;

pub trait MemPoolProvider {
    fn estimate_next_blocktime(&self) -> JoinHandle<Result<Duration>>;
    fn collect_deposit_cells(&self) -> JoinHandle<Result<Vec<DepositInfo>>>;
    fn query_available_custodians(
        &self,
        withdrawals: Vec<WithdrawalRequest>,
        last_finalized_block_number: u64,
        rollup_context: RollupContext,
    ) -> JoinHandle<Result<CollectedCustodianCells>>;
    fn get_cell(&self, out_point: OutPoint) -> JoinHandle<Result<Option<CellWithStatus>>>;
    fn query_mergeable_custodians(
        &self,
        collected_custodians: CollectedCustodianCells,
        last_finalized_block_number: u64,
    ) -> JoinHandle<Result<CollectedCustodianCells>>;
}

pub trait MemPoolErrorTxHandler {
    fn handle_error_receipt(&mut self, receipt: ErrorTxReceipt) -> JoinHandle<Result<()>>;
}
