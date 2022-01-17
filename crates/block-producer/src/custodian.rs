use anyhow::Result;
use async_trait::async_trait;
use gw_rpc_client::rpc_client::{QueryResult, RPCClient};
use gw_types::offchain::CollectedCustodianCells;

pub const MAX_CUSTODIANS: usize = 50;

#[async_trait]
pub trait MergeableCustodians {
    async fn query(
        &self,
        collected_custodians: CollectedCustodianCells,
        last_finalized_block_number: u64,
    ) -> Result<CollectedCustodianCells>;
}

pub struct DefaultMergeableCustodians {
    rpc_client: RPCClient,
}

impl DefaultMergeableCustodians {
    pub fn new(rpc_client: RPCClient) -> Self {
        DefaultMergeableCustodians { rpc_client }
    }
}

#[async_trait]
impl MergeableCustodians for DefaultMergeableCustodians {
    async fn query(
        &self,
        collected_custodians: CollectedCustodianCells,
        last_finalized_block_number: u64,
    ) -> Result<CollectedCustodianCells> {
        let query = query_mergeable_custodians(
            &self.rpc_client,
            collected_custodians,
            last_finalized_block_number,
        );
        Ok(query.await?.expect_any())
    }
}

pub async fn query_mergeable_custodians(
    rpc_client: &RPCClient,
    collected_custodians: CollectedCustodianCells,
    last_finalized_block_number: u64,
) -> Result<QueryResult<CollectedCustodianCells>> {
    if collected_custodians.cells_info.len() >= MAX_CUSTODIANS {
        return Ok(QueryResult::Full(collected_custodians));
    }

    let query_result = query_mergeable_ckb_custodians(
        rpc_client,
        collected_custodians,
        last_finalized_block_number,
    )
    .await?;
    if matches!(query_result, QueryResult::Full(_)) {
        return Ok(query_result);
    }

    query_mergeable_sudt_custodians(
        rpc_client,
        query_result.expect_any(),
        last_finalized_block_number,
    )
    .await
}

async fn query_mergeable_ckb_custodians(
    rpc_client: &RPCClient,
    collected: CollectedCustodianCells,
    last_finalized_block_number: u64,
) -> Result<QueryResult<CollectedCustodianCells>> {
    if collected.cells_info.len() >= MAX_CUSTODIANS {
        return Ok(QueryResult::Full(collected));
    }

    rpc_client
        .query_mergeable_ckb_custodians_cells(
            collected,
            last_finalized_block_number,
            MAX_CUSTODIANS,
        )
        .await
}

async fn query_mergeable_sudt_custodians(
    rpc_client: &RPCClient,
    collected: CollectedCustodianCells,
    last_finalized_block_number: u64,
) -> Result<QueryResult<CollectedCustodianCells>> {
    if collected.cells_info.len() >= MAX_CUSTODIANS {
        return Ok(QueryResult::Full(collected));
    }

    rpc_client
        .query_mergeable_sudt_custodians_cells(
            collected,
            last_finalized_block_number,
            MAX_CUSTODIANS,
        )
        .await
}
