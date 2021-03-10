use gw_types::packed::{CellOutput, DepositionRequest, OutPoint};

pub struct CellInfo {
    pub out_point: OutPoint,
    pub output: CellOutput,
}

pub struct CellCollector;

impl CellCollector {
    /// return all lived deposition requests
    pub fn query_deposition_requests(&self) -> Vec<DepositionRequest> {
        unimplemented!()
    }

    /// query lived rollup cell
    pub fn query_rollup_cell(&self) -> Option<CellInfo> {
        unimplemented!()
    }
}
