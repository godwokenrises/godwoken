use std::{
    collections::{HashMap, HashSet},
    convert::TryFrom,
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

    pub fn local_live(&self) -> impl Iterator<Item = &CellInfo> + '_ {
        self.local_live_cells.values()
    }

    /// Remove from live and add to dead.
    pub fn local_cell(&mut self, out_point: OutPoint) {
        self.local_live_cells.remove(&out_point);
        self.dead_cells.insert(out_point);
    }

    /// Add transaction inputs to dead cells, and remove them from live cells.
    ///
    /// And add transaction outputs to live cells.
    pub fn apply_tx(&mut self, tx: &TransactionReader) {
        for input in tx.raw().inputs().iter() {
            let out_point = input.previous_output().to_entity();
            self.local_cell(out_point);
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
