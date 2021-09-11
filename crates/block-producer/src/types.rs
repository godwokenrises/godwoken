use std::fmt::{self, Display};

use gw_types::{
    packed::{Block, NumberHash},
    prelude::Unpack,
};

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

impl Display for ChainEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NewBlock { block } => {
                write!(f, "ChainEvent::NewBlock{{")?;
                write!(
                    f,
                    "block: <number: {}, hash: {}>",
                    block.header().raw().number().unpack(),
                    hex::encode(block.header().hash())
                )?;
                write!(f, "}}")
            }
            Self::Reverted { old_tip, new_block } => {
                write!(f, "ChainEvent::Reverted{{")?;
                write!(
                    f,
                    "old_tip: <number: {}, hash: {}>",
                    old_tip.number().unpack(),
                    old_tip.block_hash()
                )?;
                write!(
                    f,
                    "new_block: <number: {}, hash: {}>",
                    new_block.header().raw().number().unpack(),
                    hex::encode(new_block.header().hash())
                )?;
                write!(f, "}}")
            }
        }
    }
}
