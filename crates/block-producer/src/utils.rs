use crate::debugger;
use anyhow::{anyhow, Result};
use async_jsonrpc_client::Output;
use gw_config::DebugConfig;
use gw_rpc_client::rpc_client::RPCClient;
use gw_types::packed::Transaction;
use serde::de::DeserializeOwned;
use serde_json::from_value;
use std::path::Path;

// convert json output to result
pub fn to_result<T: DeserializeOwned>(output: Output) -> Result<T> {
    match output {
        Output::Success(success) => Ok(from_value(success.result)?),
        Output::Failure(failure) => Err(anyhow!("JSONRPC error: {}", failure.error)),
    }
}

pub async fn dry_run_transaction(
    debug_config: &DebugConfig,
    rpc_client: &RPCClient,
    tx: Transaction,
    action: &str,
) -> Option<u64> {
    if debug_config.output_l1_tx_cycles {
        let dry_run_result = rpc_client.dry_run_transaction(tx.clone()).await;
        match dry_run_result {
            Ok(cycles) => {
                log::info!(
                    "Tx({}) {} execution cycles: {}",
                    action,
                    hex::encode(tx.hash()),
                    cycles
                );
                return Some(cycles);
            }
            Err(err) => log::error!(
                "Fail to dry run transaction {}, error: {}",
                hex::encode(tx.hash()),
                err
            ),
        }
    }
    None
}

pub async fn dump_transaction<P: AsRef<Path>>(dir: P, rpc_client: &RPCClient, tx: Transaction) {
    if let Err(err) = debugger::dump_transaction(dir, rpc_client, tx.clone()).await {
        log::error!(
            "Faild to dump transaction {} error: {}",
            hex::encode(&tx.hash()),
            err
        );
    }
}
