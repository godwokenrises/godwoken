use anyhow::Result;
use gw_block_producer::custodian::MergeableCustodians;
use gw_types::offchain::CollectedCustodianCells;

#[derive(Debug, Default)]
pub struct DummyMergeableCustodians {}

#[gw_mem_pool::async_trait]
impl MergeableCustodians for DummyMergeableCustodians {
    async fn query(
        &self,
        collected_custodians: CollectedCustodianCells,
        _last_finalized_block_number: u64,
    ) -> Result<CollectedCustodianCells> {
        Ok(collected_custodians)
    }
}
