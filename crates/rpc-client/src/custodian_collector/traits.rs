use anyhow::Result;
use gw_jsonrpc_types::ckb_jsonrpc_types::JsonBytes;
use gw_types::packed::Byte32;

use crate::indexer_types::{Cell, Pagination, SearchKey};

pub trait Collector {
    fn build_search_key(&self) -> SearchKey;
    fn l1_sudt_script_type_hash(&self) -> Byte32;
    fn get_cells(
        &self,
        search_key: &SearchKey,
        cursor: Option<JsonBytes>,
    ) -> Result<Pagination<Cell>>;
}
