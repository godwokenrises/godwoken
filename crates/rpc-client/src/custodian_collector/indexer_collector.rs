use super::traits::Collector;

use crate::{
    indexer_types::{Cell, Order, Pagination, ScriptType, SearchKey},
    rpc_client::RPCClient,
    utils::{to_result, DEFAULT_QUERY_LIMIT},
};
use anyhow::Result;
use async_jsonrpc_client::{Params as ClientParams, Transport};
use gw_jsonrpc_types::ckb_jsonrpc_types::{JsonBytes, Uint32};
use gw_types::{
    packed::{Byte32, Script},
    prelude::*,
};
use serde_json::json;

pub struct IndexerCollector<'a> {
    pub rpc_client: &'a RPCClient,
}

impl<'a> Collector for IndexerCollector<'a> {
    fn build_search_key(&self) -> SearchKey {
        let rollup_context = &self.rpc_client.rollup_context;

        let custodian_lock = Script::new_builder()
            .code_hash(rollup_context.rollup_config.custodian_script_type_hash())
            .hash_type(ckb_types::core::ScriptHashType::Type.into())
            .args(rollup_context.rollup_script_hash.as_slice().pack())
            .build();
        SearchKey {
            script: ckb_types::packed::Script::new_unchecked(custodian_lock.as_bytes()).into(),
            script_type: ScriptType::Lock,
            filter: None,
        }
    }

    fn l1_sudt_script_type_hash(&self) -> Byte32 {
        self.rpc_client
            .rollup_context
            .rollup_config
            .l1_sudt_script_type_hash()
    }

    fn get_cells(
        &self,
        search_key: &SearchKey,
        cursor: Option<JsonBytes>,
    ) -> Result<Pagination<Cell>> {
        let order = Order::Asc;
        let limit = Uint32::from(DEFAULT_QUERY_LIMIT as u32);

        let cells: Pagination<Cell> =
            to_result(smol::block_on(self.rpc_client.indexer.client().request(
                "get_cells",
                Some(ClientParams::Array(vec![
                    json!(search_key),
                    json!(order),
                    json!(limit),
                    json!(cursor),
                ])),
            ))?)?;
        Ok(cells)
    }
}
