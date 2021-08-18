use anyhow::Result;
use gw_types::{
    offchain::{DepositInfo, RollupContext},
    packed::WithdrawalRequest,
};
use smol::Task;

use crate::custodian::AvailableCustodians;

pub trait MemPoolProvider {
    fn estimate_next_blocktime(&self) -> Task<Result<u64>>;
    fn collect_deposit_cells(&self) -> Task<Result<Vec<DepositInfo>>>;
    fn query_available_custodians(
        &self,
        withdrawals: Vec<WithdrawalRequest>,
        last_finalized_block_number: u64,
        rollup_context: RollupContext,
    ) -> Task<Result<AvailableCustodians>>;
}
