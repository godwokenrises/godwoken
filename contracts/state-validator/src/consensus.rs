use crate::context::Context;
use crate::error::Error;
use gw_common::state::State;

const CKB_TOKEN_ID: [u8; 32] = [0u8; 32];
const REQUIRED_BALANCE: u128 = 50000_00000000u128;

/// Verify aggregator
pub fn verify_aggregator(context: &Context) -> Result<(), Error> {
    // any account has enough balance can become an aggregator
    let balance = context.get_sudt_balance(&CKB_TOKEN_ID, context.aggregator_id)?;
    if balance < REQUIRED_BALANCE {
        return Err(Error::Aggregator);
    }

    Ok(())
}
