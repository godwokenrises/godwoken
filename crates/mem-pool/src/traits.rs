use std::time::Duration;

use anyhow::Result;
use gw_types::{
    offchain::{
        CellWithStatus, CollectedCustodianCells, DepositInfo, ErrorTxReceipt, RollupContext,
    },
    packed::{OutPoint, WithdrawalRequest},
};
use smol::Task;

pub trait MemPoolProvider {
    fn estimate_next_blocktime(&self) -> Task<Result<Duration>>;
    fn collect_deposit_cells(&self) -> Task<Result<Vec<DepositInfo>>>;
    fn query_available_custodians(
        &self,
        withdrawals: Vec<WithdrawalRequest>,
        last_finalized_block_number: u64,
        rollup_context: RollupContext,
    ) -> Task<Result<CollectedCustodianCells>>;
    fn get_cell(&self, out_point: OutPoint) -> Task<Result<Option<CellWithStatus>>>;
}

pub trait MemPoolErrorTxHandler {
    fn handle_error_receipt(&mut self, receipt: ErrorTxReceipt) -> Task<Result<()>>;
}
