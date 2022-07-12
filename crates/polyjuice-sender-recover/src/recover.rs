use anyhow::Result;
use gw_types::offchain::RollupContext;
use gw_utils::wallet::Wallet;

pub mod error;
pub mod eth_account_creator;
pub mod eth_recover;
pub mod eth_sender;
use eth_recover::EthRecover;

pub struct PolyjuiceSenderRecover {
    pub eth: EthRecover,
}

impl PolyjuiceSenderRecover {
    pub fn create(rollup_context: &RollupContext, creator_wallet: Option<Wallet>) -> Result<Self> {
        let eth = EthRecover::create(rollup_context, creator_wallet)?;

        Ok(Self { eth })
    }
}
