use std::time::Duration;

use anyhow::Result;
use gw_mem_pool::traits::MemPoolProvider;
use gw_runtime::spawn;
use gw_types::{
    offchain::{CellStatus, CellWithStatus, CollectedCustodianCells, DepositInfo, RollupContext},
    packed::{OutPoint, WithdrawalRequest},
};
use tokio::task::JoinHandle;

#[derive(Debug, Default)]
pub struct DummyMemPoolProvider {
    pub fake_blocktime: Duration,
    pub deposit_cells: Vec<DepositInfo>,
    pub collected_custodians: CollectedCustodianCells,
}

impl MemPoolProvider for DummyMemPoolProvider {
    fn estimate_next_blocktime(&self) -> JoinHandle<Result<Duration>> {
        let fake_blocktime = self.fake_blocktime;
        spawn(async move { Ok(fake_blocktime) })
    }
    fn collect_deposit_cells(&self) -> JoinHandle<Result<Vec<DepositInfo>>> {
        let deposit_cells = self.deposit_cells.clone();
        spawn(async move { Ok(deposit_cells) })
    }
    fn query_available_custodians(
        &self,
        _withdrawals: Vec<WithdrawalRequest>,
        _last_finalized_block_number: u64,
        _rollup_context: RollupContext,
    ) -> JoinHandle<Result<CollectedCustodianCells>> {
        let collected_custodians = self.collected_custodians.clone();
        spawn(async move { Ok(collected_custodians) })
    }
    fn get_cell(&self, _out_point: OutPoint) -> JoinHandle<Result<Option<CellWithStatus>>> {
        spawn(async {
            Ok(Some(CellWithStatus {
                cell: Some(Default::default()),
                status: CellStatus::Live,
            }))
        })
    }
    fn query_mergeable_custodians(
        &self,
        collected_custodians: CollectedCustodianCells,
        _last_finalized_block_number: u64,
    ) -> JoinHandle<Result<CollectedCustodianCells>> {
        spawn(async move { Ok(collected_custodians) })
    }
}
