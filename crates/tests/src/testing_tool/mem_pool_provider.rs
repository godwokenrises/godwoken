use std::time::Duration;

use anyhow::Result;
use gw_mem_pool::traits::MemPoolProvider;
use gw_types::{
    offchain::{CellStatus, CellWithStatus, CollectedCustodianCells, DepositInfo, RollupContext},
    packed::{OutPoint, WithdrawalRequest},
};

#[derive(Debug, Default)]
pub struct DummyMemPoolProvider {
    pub fake_blocktime: Duration,
    pub deposit_cells: Vec<DepositInfo>,
    pub collected_custodians: CollectedCustodianCells,
}

#[gw_mem_pool::async_trait]
impl MemPoolProvider for DummyMemPoolProvider {
    async fn estimate_next_blocktime(&self) -> Result<Duration> {
        Ok(self.fake_blocktime)
    }
    async fn collect_deposit_cells(&self) -> Result<Vec<DepositInfo>> {
        Ok(self.deposit_cells.clone())
    }
    async fn query_available_custodians(
        &self,
        _withdrawals: Vec<WithdrawalRequest>,
        _last_finalized_block_number: u64,
        _rollup_context: RollupContext,
    ) -> Result<CollectedCustodianCells> {
        Ok(self.collected_custodians.clone())
    }
    async fn get_cell(&self, _out_point: OutPoint) -> Result<Option<CellWithStatus>> {
        Ok(Some(CellWithStatus {
            cell: Some(Default::default()),
            status: CellStatus::Live,
        }))
    }
    async fn query_mergeable_custodians(
        &self,
        collected_custodians: CollectedCustodianCells,
        _last_finalized_block_number: u64,
    ) -> Result<CollectedCustodianCells> {
        Ok(collected_custodians)
    }
}
