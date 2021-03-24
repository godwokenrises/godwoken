use gw_types::{
    bytes::Bytes,
    packed::{CellDep, CellInput, CellOutput, OutPoint},
};

#[derive(Clone)]
pub struct CellInfo {
    pub out_point: OutPoint,
    pub output: CellOutput,
    pub data: Bytes,
    pub lock_dep: CellDep,
    pub type_dep: Option<CellDep>,
}

pub struct InputCellInfo {
    pub input: CellInput,
    pub cell: CellInfo,
}

#[derive(Clone)]
pub struct SignatureEntry {
    pub indexes: Vec<usize>,
    pub lock_hash: [u8; 32],
}
