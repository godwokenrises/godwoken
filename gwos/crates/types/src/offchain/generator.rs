#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CycleMeter {
    pub execution: u64,
    pub r#virtual: u64,
}

impl CycleMeter {
    pub fn total(&self) -> u64 {
        self.execution.saturating_add(self.r#virtual)
    }
}
