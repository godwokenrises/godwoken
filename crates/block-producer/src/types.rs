use gw_types::packed::{Block, NumberHash};

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
