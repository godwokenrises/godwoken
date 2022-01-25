use anyhow::Result;
use ckb_jsonrpc_types::JsonBytes;

use crate::{account::parse_account_short_script_hash, godwoken_rpc::GodwokenRpcClient};

pub fn get_balance(godwoken_rpc_url: &str, account: &str, sudt_id: u32) -> Result<()> {
    let mut godwoken_rpc_client = GodwokenRpcClient::new(godwoken_rpc_url);
    let short_script_hash = parse_account_short_script_hash(&mut godwoken_rpc_client, account)?;
    let addr = JsonBytes::from_bytes(short_script_hash);
    let balance = godwoken_rpc_client.get_balance(addr, sudt_id)?;
    log::info!("Balance: {}", balance);

    Ok(())
}
