use std::time::Duration;

use anyhow::Result;
use gw_mem_pool::traits::MemPoolProvider;
use gw_types::{
    offchain::{CollectedCustodianCells, DepositInfo, RollupContext},
    packed::WithdrawalRequest,
};
use smol::Task;

#[derive(Debug, Default)]
pub struct DummyMemPoolProvider {
    pub deposit_cells: Vec<DepositInfo>,
    pub collected_custodians: CollectedCustodianCells,
}

impl MemPoolProvider for DummyMemPoolProvider {
    fn estimate_next_blocktime(&self, last: Option<Duration>) -> Task<Result<Duration>> {
        let fake_blocktime = last
            .unwrap_or_else(|| Duration::from_secs(0))
            .checked_add(Duration::from_secs(1))
            .unwrap();
        smol::spawn(async move { Ok(fake_blocktime) })
    }
    fn collect_deposit_cells(&self) -> Task<Result<Vec<DepositInfo>>> {
        let deposit_cells = self.deposit_cells.clone();
        smol::spawn(async move { Ok(deposit_cells) })
    }
    fn query_available_custodians(
        &self,
        _withdrawals: Vec<WithdrawalRequest>,
        _last_finalized_block_number: u64,
        _rollup_context: RollupContext,
    ) -> Task<Result<CollectedCustodianCells>> {
        let collected_custodians = self.collected_custodians.clone();
        smol::spawn(async move { Ok(collected_custodians) })
    }
}
