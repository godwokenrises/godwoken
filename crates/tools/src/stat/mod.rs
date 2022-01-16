use anyhow::Result;
use ckb_types::prelude::{Builder, Entity};
use gw_common::H256;
use gw_rpc_client::indexer_client::CKBIndexerClient;
use gw_types::{core::ScriptHashType, offchain::CustodianStat, packed::Script, prelude::Pack};

/// Query custodian ckb from ckb-indexer
pub async fn stat_custodian_cells(
    rpc_client: &CKBIndexerClient,
    rollup_type_hash: &H256,
    custodian_script_type_hash: &H256,
    min_capacity: Option<u64>,
    last_finalized_block_number: u64,
) -> Result<CustodianStat> {
    let script = Script::new_builder()
        .code_hash(custodian_script_type_hash.pack())
        .hash_type(ScriptHashType::Type.into())
        .args(rollup_type_hash.as_slice().to_vec().pack())
        .build();
    rpc_client
        .stat_custodian_cells(script, min_capacity, last_finalized_block_number)
        .await
}
