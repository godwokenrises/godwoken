use crate::indexer_client::CKBIndexerClient;
use crate::indexer_types::{Cell, Order, Pagination, SearchKey};

use anyhow::Result;
use async_jsonrpc_client::Params;
use async_trait::async_trait;
use gw_jsonrpc_types::ckb_jsonrpc_types::{JsonBytes, Uint32};
use serde_json::json;

#[async_trait]
pub trait IndexedCells {
    async fn get_cells(
        &self,
        search_key: &SearchKey,
        order: &Order,
        limit: &Uint32,
        cursor: Option<JsonBytes>,
    ) -> Result<Pagination<Cell>>;
}

#[async_trait]
impl IndexedCells for CKBIndexerClient {
    async fn get_cells(
        &self,
        search_key: &SearchKey,
        order: &Order,
        limit: &Uint32,
        cursor: Option<JsonBytes>,
    ) -> Result<Pagination<Cell>> {
        self.request(
            "get_cells",
            Some(Params::Array(vec![
                json!(search_key),
                json!(order),
                json!(limit),
                json!(cursor),
            ])),
        )
        .await
    }
}
