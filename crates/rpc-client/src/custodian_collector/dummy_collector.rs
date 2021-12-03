use super::traits::Collector;

use crate::indexer_types::{Cell, Pagination, ScriptType, SearchKey};
use anyhow::Result;
use gw_jsonrpc_types::ckb_jsonrpc_types::JsonBytes;
use gw_types::packed::Byte32;

pub struct DummyCollector {
    pub l1_sudt_script_type_hash: Byte32,
}

impl Collector for DummyCollector {
    fn build_search_key(&self) -> SearchKey {
        SearchKey {
            script: Default::default(),
            script_type: ScriptType::Lock,
            filter: None,
        }
    }

    fn l1_sudt_script_type_hash(&self) -> Byte32 {
        self.l1_sudt_script_type_hash.clone()
    }

    fn get_cells(
        &self,
        _search_key: &SearchKey,
        _cursor: Option<JsonBytes>,
    ) -> Result<Pagination<Cell>> {
        Ok(Pagination {
            objects: vec![],
            last_cursor: JsonBytes::default(),
        })
    }
}
