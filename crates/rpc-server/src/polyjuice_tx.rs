pub mod eth_account_creator;
pub mod eth_context;
pub mod eth_sender;
use eth_context::EthContext;

pub const ERR_UNREGISTERED_EOA_ACCOUNT: &str = "unregistered EOA account";

pub struct PolyjuiceTxContext {
    pub eth: EthContext,
}

impl PolyjuiceTxContext {
    pub fn new(eth: EthContext) -> Self {
        Self { eth }
    }
}
