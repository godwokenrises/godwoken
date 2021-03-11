use anyhow::{anyhow, Result};
use gw_types::packed::{CellDep, CellOutput, DepositionRequest, OutPoint, Transaction};

pub struct CellInfo {
    pub out_point: OutPoint,
    pub output: CellOutput,
    pub lock_dep: CellDep,
    pub type_dep: Option<CellDep>,
}

pub struct DepositInfo {
    pub request: DepositionRequest,
    pub cell: CellInfo,
}

pub struct CellCollector;

impl CellCollector {
    /// return all lived deposition requests
    pub fn query_deposit_cells(&self) -> Vec<DepositInfo> {
        unimplemented!()
    }

    /// query lived rollup cell
    pub fn query_rollup_cell(&self) -> Option<CellInfo> {
        unimplemented!()
    }

    pub fn send_transaction(&self, tx: Transaction) -> Result<()> {
        unimplemented!()
    }
}
