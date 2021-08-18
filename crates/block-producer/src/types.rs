use gw_types::packed::{Block, NumberHash};

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
