use anyhow::Result;
use gw_rpc_client::indexer_client::CkbIndexerClient;
use gw_types::{
    core::ScriptHashType,
    h256::*,
    offchain::{CompatibleFinalizedTimepoint, CustodianStat},
    packed::Script,
    prelude::*,
};

/// Query custodian ckb from ckb-indexer
pub async fn stat_custodian_cells(
    rpc_client: &CkbIndexerClient,
    rollup_type_hash: &H256,
    custodian_script_type_hash: &H256,
    min_capacity: Option<u64>,
    compatible_finalized_timepoint: &CompatibleFinalizedTimepoint,
) -> Result<CustodianStat> {
    let script = Script::new_builder()
        .code_hash(custodian_script_type_hash.pack())
        .hash_type(ScriptHashType::Type.into())
        .args(rollup_type_hash.as_slice().to_vec().pack())
        .build();
    rpc_client
        .stat_custodian_cells(script, min_capacity, compatible_finalized_timepoint)
        .await
}
