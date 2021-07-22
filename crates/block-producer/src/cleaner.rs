use crate::rpc_client::RPCClient;

use anyhow::Result;
use gw_chain::chain::Chain;

use std::sync::Arc;

pub struct Cleaner {
    rpc_client: RPCClient,
    chain: Arc<parking_lot::Mutex<Chain>>,
}

// TODO: prune db state
impl Cleaner {
    pub fn new(rpc_client: RPCClient, chain: Arc<parking_lot::Mutex<Chain>>) -> Self {
        Cleaner { rpc_client, chain }
    }
}
