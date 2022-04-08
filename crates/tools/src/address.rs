use std::path::Path;

use crate::{
    godwoken_rpc::GodwokenRpcClient, types::ScriptsDeploymentResult,
    utils::transaction::read_config,
};
use anyhow::{anyhow, Result};
use ckb_jsonrpc_types::JsonBytes;
use ckb_types::{
    core::ScriptHashType,
    prelude::{Builder, Entity},
};
use gw_types::{bytes::Bytes as GwBytes, packed::Script, prelude::Pack as GwPack};

pub fn to_godwoken_short_script_hash(
    eth_eoa_address: &str,
    config_path: &Path,
    scripts_deployment_path: &Path,
) -> Result<()> {
    if eth_eoa_address.len() != 42 || !eth_eoa_address.starts_with("0x") {
        return Err(anyhow!("eth eoa address format error!"));
    }

    let eth_eoa_addr = GwBytes::from(hex::decode(
        eth_eoa_address.trim_start_matches("0x").as_bytes(),
    )?);

    let config = read_config(&config_path)?;
    let rollup_type_hash = &config.genesis.rollup_type_hash;

    let scripts_deployment_content = std::fs::read_to_string(scripts_deployment_path)?;
    let scripts_deployment: ScriptsDeploymentResult =
        serde_json::from_str(&scripts_deployment_content)?;

    let l2_code_hash = &scripts_deployment.eth_account_lock.script_type_hash;
    let mut l2_args_vec = rollup_type_hash.as_bytes().to_vec();
    l2_args_vec.append(&mut eth_eoa_addr.to_vec());
    let l2_lock_args = GwBytes::from(l2_args_vec);

    let l2_lock = Script::new_builder()
        .code_hash(l2_code_hash.pack())
        .hash_type(ScriptHashType::Type.into())
        .args(l2_lock_args.pack())
        .build();

    let l2_lock_hash = l2_lock.hash();
    let short_script_hash = &l2_lock_hash[..20];

    log::info!(
        "godwoken short script hash: 0x{}",
        hex::encode(short_script_hash)
    );

    Ok(())
}

pub async fn to_eth_eoa_address(godwoken_rpc_url: &str, godwoken_short_script_hash: &str) -> Result<()> {
    if godwoken_short_script_hash.len() != 42 || !godwoken_short_script_hash.starts_with("0x") {
        return Err(anyhow!("godwoken short script hash format error!"));
    }

    let godwoken_rpc_client = GodwokenRpcClient::new(godwoken_rpc_url);

    let short_script_hash = GwBytes::from(hex::decode(
        godwoken_short_script_hash
            .trim_start_matches("0x")
            .as_bytes(),
    )?);

    let script_hash = godwoken_rpc_client
        .get_script_hash_by_short_script_hash(JsonBytes::from_bytes(short_script_hash)).await?;

    let script = match script_hash {
        Some(h) => godwoken_rpc_client.get_script(h).await?,
        None => return Err(anyhow!("script hash not found!")),
    };

    let args = match script {
        Some(s) => s.args,
        None => return Err(anyhow!("script not found!")),
    };

    let eth_address = &args.as_bytes()[32..];

    log::info!("eth eoa address: 0x{}", hex::encode(eth_address));

    Ok(())
}
