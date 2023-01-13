use anyhow::Result;
use gw_rpc_client::{
    indexer_types::{Order, SearchKey},
    rpc_client::RPCClient,
};
use gw_types::offchain::CellInfo;

use crate::local_cells::{
    collect_local_and_indexer_cells, CollectLocalAndIndexerCursor, LocalCellsManager,
};

pub async fn query_rollup_cell(
    local_cells_manager: &LocalCellsManager,
    rpc_client: &RPCClient,
) -> Result<Option<CellInfo>> {
    let search_key = SearchKey::with_type(rpc_client.rollup_type_script.clone());
    let mut cursor = CollectLocalAndIndexerCursor::Local;
    while !cursor.is_ended() {
        let mut cells = collect_local_and_indexer_cells(
            local_cells_manager,
            &rpc_client.indexer,
            &search_key,
            &Order::Desc,
            Some(1),
            &mut cursor,
        )
        .await?;
        if !cells.is_empty() {
            return Ok(Some(cells.swap_remove(0)));
        }
    }
    Ok(None)
}
