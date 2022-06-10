use std::time::Duration;

use anyhow::Result;
use async_trait::async_trait;
use gw_types::{
    offchain::{CellWithStatus, DepositInfo, ErrorTxReceipt},
    packed::OutPoint,
};
use gw_utils::local_cells::LocalCellsManager;

#[async_trait]
pub trait MemPoolProvider {
    async fn estimate_next_blocktime(&self) -> Result<Duration>;
    async fn collect_deposit_cells(
        &self,
        local_cells_manager: &LocalCellsManager,
    ) -> Result<Vec<DepositInfo>>;
    async fn get_cell(&self, out_point: OutPoint) -> Result<Option<CellWithStatus>>;
}

#[async_trait]
pub trait MemPoolErrorTxHandler {
    async fn handle_error_receipt(&mut self, receipt: ErrorTxReceipt) -> Result<()>;
}
