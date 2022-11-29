//! For for implement offchain operations or for testing purpose

use std::collections::{HashMap, HashSet};

use ckb_types::{
    bytes::Bytes,
    core::{HeaderView, TransactionView},
    packed::{Byte32, CellDep, CellOutput, OutPoint, Script, Transaction},
    prelude::*,
    H256,
};

use crate::utils::sdk::traits::{
    CellCollector, CellCollectorError, CellDepResolver, CellQueryOptions, HeaderDepResolver,
    LiveCell, TransactionDependencyError, TransactionDependencyProvider,
};
use crate::utils::sdk::types::ScriptId;
use anyhow::anyhow;

/// A offchain cell_dep resolver
#[derive(Default, Clone)]
pub struct OffchainCellDepResolver {
    pub items: HashMap<ScriptId, (CellDep, String)>,
}
impl CellDepResolver for OffchainCellDepResolver {
    fn resolve(&self, script: &Script) -> Option<CellDep> {
        let script_id = ScriptId::from(script);
        self.items
            .get(&script_id)
            .map(|(cell_dep, _)| cell_dep.clone())
    }
}

#[derive(Default, Clone)]
pub struct OffchainHeaderDepResolver {
    pub by_tx_hash: HashMap<H256, HeaderView>,
    pub by_number: HashMap<u64, HeaderView>,
}

impl HeaderDepResolver for OffchainHeaderDepResolver {
    fn resolve_by_tx(&self, tx_hash: &Byte32) -> Result<Option<HeaderView>, anyhow::Error> {
        let tx_hash: H256 = tx_hash.unpack();
        Ok(self.by_tx_hash.get(&tx_hash).cloned())
    }
    fn resolve_by_number(&self, number: u64) -> Result<Option<HeaderView>, anyhow::Error> {
        Ok(self.by_number.get(&number).cloned())
    }
}

/// A cell collector only use offchain data
#[derive(Default, Clone)]
pub struct OffchainCellCollector {
    pub locked_cells: HashSet<(H256, u32)>,
    pub live_cells: Vec<LiveCell>,
    pub max_mature_number: u64,
}

impl OffchainCellCollector {
    pub fn new(
        locked_cells: HashSet<(H256, u32)>,
        live_cells: Vec<LiveCell>,
        max_mature_number: u64,
    ) -> OffchainCellCollector {
        OffchainCellCollector {
            locked_cells,
            live_cells,
            max_mature_number,
        }
    }

    pub fn collect(&self, query: &CellQueryOptions) -> (Vec<LiveCell>, Vec<LiveCell>, u64) {
        let mut total_capacity = 0;
        let (cells, rest_cells): (Vec<_>, Vec<_>) =
            self.live_cells.clone().into_iter().partition(|cell| {
                if total_capacity < query.min_total_capacity
                    && query.match_cell(cell, self.max_mature_number)
                {
                    let capacity: u64 = cell.output.capacity().unpack();
                    total_capacity += capacity;
                    true
                } else {
                    false
                }
            });
        (cells, rest_cells, total_capacity)
    }
}

impl CellCollector for OffchainCellCollector {
    fn collect_live_cells(
        &mut self,
        query: &CellQueryOptions,
        apply_changes: bool,
    ) -> Result<(Vec<LiveCell>, u64), CellCollectorError> {
        let (cells, rest_cells, total_capacity) = self.collect(query);
        if apply_changes {
            self.live_cells = rest_cells;
            for cell in &cells {
                self.lock_cell(cell.out_point.clone())?;
            }
        }
        Ok((cells, total_capacity))
    }

    fn lock_cell(&mut self, out_point: OutPoint) -> Result<(), CellCollectorError> {
        self.locked_cells
            .insert((out_point.tx_hash().unpack(), out_point.index().unpack()));
        Ok(())
    }
    fn apply_tx(&mut self, tx: Transaction) -> Result<(), CellCollectorError> {
        let tx_view = tx.into_view();
        let tx_hash = tx_view.hash();
        for out_point in tx_view.input_pts_iter() {
            self.lock_cell(out_point)?;
        }
        for (output_index, (output, data)) in tx_view.outputs_with_data_iter().enumerate() {
            let out_point = OutPoint::new(tx_hash.clone(), output_index as u32);
            let info = LiveCell {
                output: output.clone(),
                output_data: data.clone(),
                out_point,
                block_number: 0,
                tx_index: 0,
            };
            self.live_cells.push(info);
        }
        Ok(())
    }

    fn reset(&mut self) {
        self.locked_cells.clear();
        self.live_cells.clear();
    }
}

/// offchain transaction dependency provider
#[derive(Default, Clone)]
pub struct OffchainTransactionDependencyProvider {
    pub txs: HashMap<H256, TransactionView>,
    pub cells: HashMap<(H256, u32), (CellOutput, Bytes)>,
    pub headers: HashMap<H256, HeaderView>,
}

impl TransactionDependencyProvider for OffchainTransactionDependencyProvider {
    // For verify certain cell belong to certain transaction
    fn get_transaction(
        &self,
        tx_hash: &Byte32,
    ) -> Result<TransactionView, TransactionDependencyError> {
        let tx_hash: H256 = tx_hash.unpack();
        self.txs
            .get(&tx_hash)
            .cloned()
            .ok_or_else(|| TransactionDependencyError::Other(anyhow!("offchain get_transaction")))
    }
    // For get the output information of inputs or cell_deps, those cell should be live cell
    fn get_cell(&self, out_point: &OutPoint) -> Result<CellOutput, TransactionDependencyError> {
        let tx_hash: H256 = out_point.tx_hash().unpack();
        let index: u32 = out_point.index().unpack();
        self.cells
            .get(&(tx_hash, index))
            .map(|(output, _)| output.clone())
            .ok_or_else(|| TransactionDependencyError::Other(anyhow!("offchain get_cell")))
    }
    // For get the output data information of inputs or cell_deps
    fn get_cell_data(&self, out_point: &OutPoint) -> Result<Bytes, TransactionDependencyError> {
        let tx_hash: H256 = out_point.tx_hash().unpack();
        let index: u32 = out_point.index().unpack();
        self.cells
            .get(&(tx_hash, index))
            .map(|(_, data)| data.clone())
            .ok_or_else(|| TransactionDependencyError::Other(anyhow!("offchain get_cell_data")))
    }
    // For get the header information of header_deps
    fn get_header(&self, block_hash: &Byte32) -> Result<HeaderView, TransactionDependencyError> {
        let block_hash: H256 = block_hash.unpack();
        self.headers
            .get(&block_hash)
            .cloned()
            .ok_or_else(|| TransactionDependencyError::Other(anyhow!("offchain get_header")))
    }
}
