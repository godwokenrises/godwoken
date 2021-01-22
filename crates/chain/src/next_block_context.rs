#[derive(Debug, Eq, PartialEq, Clone)]
pub struct NextBlockContext {
    pub block_producer_id: u32,
    pub timestamp: u64,
}
