use anyhow::Result;
use ckb_jsonrpc_types::JsonBytes;

use crate::{account::parse_account_short_address, godwoken_rpc::GodwokenRpcClient};

pub fn get_balance(godwoken_rpc_url: &str, account: &str, sudt_id: u32) -> Result<()> {
    let mut godwoken_rpc_client = GodwokenRpcClient::new(godwoken_rpc_url);
    let short_address = parse_account_short_address(&mut godwoken_rpc_client, account)?;
    let addr = JsonBytes::from_bytes(short_address);
    let balance = godwoken_rpc_client.get_balance(addr, sudt_id)?;
    log::info!("Balance: {}", balance);

    Ok(())
}
