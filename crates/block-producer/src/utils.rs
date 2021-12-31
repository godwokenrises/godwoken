use crate::debugger;
use gw_rpc_client::rpc_client::RPCClient;
use gw_types::packed::Transaction;
use std::path::Path;

pub async fn dump_transaction<P: AsRef<Path>>(dir: P, rpc_client: &RPCClient, tx: Transaction) {
    if let Err(err) = debugger::dump_transaction(dir, rpc_client, tx.clone()).await {
        log::error!(
            "Faild to dump transaction {} error: {}",
            hex::encode(&tx.hash()),
            err
        );
    }
}
