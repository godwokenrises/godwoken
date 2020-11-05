use gw_types::packed::L2Block;

#[derive(Debug)]
pub struct NextBlockContext {
    pub aggregator_id: u32,
}

pub trait Consensus {
    fn next_block_context(&self, tip: &L2Block) -> NextBlockContext;
}
