use anyhow::Result;
use ckb_types::prelude::{Builder, Entity};
use gw_common::H256;
use gw_rpc_client::indexer_client::CKBIndexerClient;
use gw_types::{core::ScriptHashType, packed::Script, prelude::Pack};

/// Query custodian ckb from ckb-indexer
pub fn query_custodian_ckb(
    rpc_client: &CKBIndexerClient,
    rollup_type_hash: &H256,
    custodian_script_type_hash: &H256,
) -> Result<u128> {
    let script = Script::new_builder()
        .code_hash(custodian_script_type_hash.pack())
        .hash_type(ScriptHashType::Type.into())
        .args(rollup_type_hash.as_slice().to_vec().pack())
        .build();
    smol::block_on(rpc_client.query_custodian_ckb(script))
}
