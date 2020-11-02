use crate::consensus::traits::{Consensus, NextBlockContext};
use gw_types::packed::L2Block;

pub struct SingleAggregator {
    aggregator_id: u32,
}

impl SingleAggregator {
    pub fn new(aggregator_id: u32) -> Self {
        SingleAggregator { aggregator_id }
    }
}

impl Consensus for SingleAggregator {
    fn next_block_context(&self, tip: &L2Block) -> NextBlockContext {
        NextBlockContext {
            aggregator_id: self.aggregator_id,
        }
    }
}
