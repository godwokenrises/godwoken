use std::{
    collections::{HashMap, HashSet},
    convert::TryFrom,
};

use anyhow::Result;
use gw_jsonrpc_types::ckb_jsonrpc_types::JsonBytes;
use gw_rpc_client::{
    indexer_client::CKBIndexerClient,
    indexer_types::{Order, ScriptType, SearchKey},
};
use gw_types::{
    bytes::Bytes,
    offchain::CellInfo,
    packed::{OutPoint, Transaction, TransactionReader},
    prelude::*,
};

/// Manage local dead / live cells.
#[derive(Default)]
pub struct LocalCellsManager {
    dead_cells: HashSet<OutPoint>,
    local_live_cells: HashMap<OutPoint, CellInfo>,
}

impl LocalCellsManager {
    pub fn is_dead(&self, out_point: &OutPoint) -> bool {
        self.dead_cells.contains(out_point)
    }

    pub fn dead_cells(&self) -> &HashSet<OutPoint> {
        &self.dead_cells
    }

    pub fn local_live(&self) -> impl Iterator<Item = &CellInfo> + '_ {
        self.local_live_cells.values()
    }

    /// Remove from live and add to dead.
    pub fn lock_cell(&mut self, out_point: OutPoint) {
        self.local_live_cells.remove(&out_point);
        self.dead_cells.insert(out_point);
    }

    pub fn add_live(&mut self, cell: CellInfo) {
        self.local_live_cells.insert(cell.out_point.clone(), cell);
    }

    /// Add transaction inputs to dead cells, and remove them from live cells.
    ///
    /// And add transaction outputs to live cells.
    pub fn apply_tx(&mut self, tx: &TransactionReader) {
        for input in tx.raw().inputs().iter() {
            let out_point = input.previous_output().to_entity();
            self.lock_cell(out_point);
        }
        let tx_hash = tx.hash().pack();
        for (idx, (output, output_data)) in tx
            .raw()
            .outputs()
            .iter()
            .zip(tx.raw().outputs_data().iter())
            .enumerate()
        {
            let out_point = OutPoint::new_builder()
                .tx_hash(tx_hash.clone())
                .index(u32::try_from(idx).unwrap().pack())
                .build();
            let cell_info = CellInfo {
                out_point: out_point.clone(),
                output: output.to_entity(),
                data: Bytes::copy_from_slice(output_data.as_slice()),
            };
            self.local_live_cells.insert(out_point, cell_info);
        }
    }

    /// Remove transaction inputs from dead cells.
    ///
    /// You should call this after the transaction has already been confirmed by
    /// ckb/ckb-indexer.
    pub fn confirm_tx(&mut self, tx: &Transaction) {
        for input in tx.raw().inputs() {
            self.dead_cells.remove(&input.previous_output());
        }
    }

    pub fn reset(&mut self) {
        self.local_live_cells.clear();
        self.dead_cells.clear();
    }
}

pub enum CollectLocalAndIndexerCursor {
    Local,
    Indexer(Option<JsonBytes>),
    Ended,
}

impl CollectLocalAndIndexerCursor {
    pub fn is_ended(&self) -> bool {
        matches!(self, CollectLocalAndIndexerCursor::Ended)
    }
}

/// Collect live cells from local cells manager and ckb-indexer.
///
/// Cells from local cells manager are returned first regardless of the `order`
/// parameter.
///
/// Cells from local cells manager are always returned regardless of the
/// `block_range` filter.
///
/// If you do not want local live cells, you can start with cursor
/// `CollectLocalAndIndexerCursor::Indexer(None)`.
///
/// Cells that are returned from the indexer but are considered dead by the
/// local cells manager are not returned.
pub async fn collect_local_and_indexer_cells(
    local_cells_manager: &LocalCellsManager,
    indexer_client: &CKBIndexerClient,
    search_key: &SearchKey,
    order: &Order,
    limit: Option<u32>,
    cursor: &mut CollectLocalAndIndexerCursor,
) -> Result<Vec<CellInfo>> {
    match cursor {
        CollectLocalAndIndexerCursor::Local => {
            let local = local_cells_manager
                .local_live()
                .filter(|c| satisfy_search(search_key, *c))
                .cloned()
                .collect();
            *cursor = CollectLocalAndIndexerCursor::Indexer(None);
            Ok(local)
        }
        CollectLocalAndIndexerCursor::Indexer(ref mut indexer_cursor) => {
            let result = indexer_client
                .get_cells(search_key, order, limit.map(Into::into), indexer_cursor)
                .await?;
            if result.last_cursor.is_empty() {
                *cursor = CollectLocalAndIndexerCursor::Ended;
            } else {
                *indexer_cursor = Some(result.last_cursor);
            }
            Ok(result
                .objects
                .into_iter()
                .filter_map(|c| {
                    let info = c.info();
                    if local_cells_manager.is_dead(&info.out_point) {
                        None
                    } else {
                        Some(info)
                    }
                })
                .collect())
        }
        CollectLocalAndIndexerCursor::Ended => Ok(Vec::new()),
    }
}

/// Check that a cell satisfy a SearchKey, in the same way as ckb-indexer,
/// except for the block_range filter, which is ignored.
fn satisfy_search(search_key: &SearchKey, c: &CellInfo) -> bool {
    let cell_script = match search_key.script_type {
        ScriptType::Lock => Some(c.output.lock()),
        ScriptType::Type => c.output.type_().to_opt(),
    };
    if !cell_script.map_or(false, |s| script_prefix_eq(&search_key.script, &s)) {
        return false;
    }

    if let Some(f) = search_key.filter.as_ref() {
        if let Some(s) = f.script.as_ref() {
            let other_script = match search_key.script_type {
                ScriptType::Type => Some(c.output.lock()),
                ScriptType::Lock => c.output.type_().to_opt(),
            };
            if !other_script.map_or(false, |o| script_prefix_eq(s, &o)) {
                return false;
            }
        }
        if let Some([start, end]) = f.output_capacity_range {
            let cap = c.output.capacity().unpack();
            if !(u64::from(start) <= cap && cap < u64::from(end)) {
                return false;
            }
        }
        if let Some([start, end]) = f.output_data_len_range {
            let len = c.data.len() as u64;
            if !(u64::from(start) <= len && len < u64::from(end)) {
                return false;
            }
        }
    }

    true
}

/// Check that this is a prefix of other.
fn script_prefix_eq(
    this: &gw_jsonrpc_types::ckb_jsonrpc_types::Script,
    other: &gw_types::packed::Script,
) -> bool {
    (this.code_hash.as_bytes(), this.hash_type.clone() as u8)
        == (other.code_hash().as_slice(), u8::from(other.hash_type()))
        && other.args().raw_data().starts_with(this.args.as_bytes())
}
