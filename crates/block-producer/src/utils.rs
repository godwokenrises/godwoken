use crate::debugger;
use gw_rpc_client::rpc_client::RPCClient;
use gw_types::core::Timepoint;
use gw_types::packed::{GlobalState, Transaction};
use gw_types::prelude::*;
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
            // the since is used to prove finality, so since value can be 1 second later
            // we adjust the value as `time_ms / 1000 + 1` to prevent the `since` in seconds is less than `time_ms`,
            Since::new_timestamp_seconds(time_ms / 1000 + 1).as_u64()
        }
    }
}
