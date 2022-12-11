use crate::debugger;
use gw_rpc_client::rpc_client::RPCClient;
use gw_types::core::Timepoint;
use gw_types::packed::{GlobalState, Transaction};
use gw_types::prelude::Unpack;
use gw_utils::since::Since;
use std::path::Path;

pub async fn dump_transaction<P: AsRef<Path>>(dir: P, rpc_client: &RPCClient, tx: &Transaction) {
    if let Err(err) = debugger::dump_transaction(dir, rpc_client, tx).await {
        log::error!(
            "Failed to dump transaction {} error: {}",
            hex::encode(&tx.hash()),
            err
        );
    }
}

/// Convert global_state.last_finalized_timepoint to the form fo Since.
pub fn global_state_last_finalized_timepoint_to_since(global_state: &GlobalState) -> u64 {
    match Timepoint::from_full_value(global_state.last_finalized_timepoint().unpack()) {
        Timepoint::BlockNumber(_) => 0,
        Timepoint::Timestamp(time_ms) => {
            // ATTENTION: I am not sure if I am do right. Please review intensively.
            Since::new_timestamp_seconds(time_ms.saturating_div(1000).saturating_sub(1)).as_u64()
        }
    }
}
