#[derive(Debug, Eq, PartialEq, Clone)]
pub struct NextBlockContext {
    pub aggregator_id: u32,
    pub timestamp: u64,
}
