use std::time::Duration;

use anyhow::Result;
use gw_mem_pool::traits::MemPoolProvider;
use gw_types::{
    offchain::{CellStatus, CellWithStatus, DepositInfo, FinalizedCustodianCapacity},
    packed::OutPoint,
};
use gw_utils::local_cells::LocalCellsManager;

#[derive(Debug, Default)]
pub struct DummyMemPoolProvider {
    pub fake_blocktime: Duration,
    pub deposit_cells: Vec<DepositInfo>,
    pub deposit_custodians: FinalizedCustodianCapacity,
}

#[gw_mem_pool::async_trait]
impl MemPoolProvider for DummyMemPoolProvider {
    async fn estimate_next_blocktime(&self) -> Result<Duration> {
        Ok(self.fake_blocktime)
    }
    async fn collect_deposit_cells(
        &self,
        _local_cells_manager: &LocalCellsManager,
    ) -> Result<Vec<DepositInfo>> {
        Ok(self.deposit_cells.clone())
    }
    fn query_block_deposit_custodians(&self, _block: u64) -> Result<FinalizedCustodianCapacity> {
        Ok(self.deposit_custodians.clone())
    }
    async fn get_cell(&self, _out_point: OutPoint) -> Result<Option<CellWithStatus>> {
        Ok(Some(CellWithStatus {
            cell: Some(Default::default()),
            status: CellStatus::Live,
        }))
    }
}
