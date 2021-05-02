use gw_types::{
    bytes::Bytes,
    packed::{Block, CellInput, CellOutput, NumberHash, OutPoint},
};

#[derive(Debug, Clone)]
pub struct CellInfo {
    pub out_point: OutPoint,
    pub output: CellOutput,
    pub data: Bytes,
}

#[derive(Debug, Clone)]
pub struct InputCellInfo {
    pub input: CellInput,
    pub cell: CellInfo,
}

#[derive(Clone)]
pub struct SignatureEntry {
    pub indexes: Vec<usize>,
    pub lock_hash: [u8; 32],
}

#[derive(Debug, Clone)]
pub enum ChainEvent {
    NewBlock {
        block: Block,
    },
    Reverted {
        old_tip: NumberHash,
        new_block: Block,
    },
}
