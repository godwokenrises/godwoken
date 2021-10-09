use std::path::Path;

use crate::{
    godwoken_rpc::GodwokenRpcClient, types::ScriptsDeploymentResult,
    utils::transaction::read_config,
};
use ckb_jsonrpc_types::JsonBytes;
use ckb_types::{
    core::ScriptHashType,
    prelude::{Builder, Entity},
};
use gw_types::{bytes::Bytes as GwBytes, packed::Script, prelude::Pack as GwPack};

pub fn to_godwoken_short_address(
    eth_eoa_address: &str,
    config_path: &Path,
    deployment_results_path: &Path,
) -> Result<(), String> {
    if eth_eoa_address.len() != 42 || !eth_eoa_address.starts_with("0x") {
        return Err("eth eoa address format error!".to_owned());
    }

    let eth_eoa_addr =
        GwBytes::from(hex::decode(eth_eoa_address[2..].as_bytes()).map_err(|err| err.to_string())?);

    let config = read_config(&config_path)?;
    let rollup_type_hash = &config.genesis.rollup_type_hash;

    let deployment_result_string =
        std::fs::read_to_string(deployment_results_path).map_err(|err| err.to_string())?;
    let deployment_result: ScriptsDeploymentResult =
        serde_json::from_str(&deployment_result_string).map_err(|err| err.to_string())?;

    let l2_code_hash = &deployment_result.eth_account_lock.script_type_hash;
    let mut l2_args_vec = rollup_type_hash.as_bytes().to_vec();
    l2_args_vec.append(&mut eth_eoa_addr.to_vec());
    let l2_lock_args = GwBytes::from(l2_args_vec);

    let l2_lock = Script::new_builder()
        .code_hash(l2_code_hash.pack())
        .hash_type(ScriptHashType::Type.into())
        .args(l2_lock_args.pack())
        .build();

    let l2_lock_hash = l2_lock.hash();
    let godwoken_address = &l2_lock_hash[..20];

    log::info!("godwoken address: 0x{}", hex::encode(godwoken_address));

    Ok(())
}

pub fn to_eth_eoa_address(
    godwoken_rpc_url: &str,
    godwoken_short_address: &str,
) -> Result<(), String> {
    if godwoken_short_address.len() != 42 || !godwoken_short_address.starts_with("0x") {
        return Err("godwoken short address format error!".to_owned());
    }

    let mut godwoken_rpc_client = GodwokenRpcClient::new(godwoken_rpc_url);

    let short_address = GwBytes::from(
        hex::decode(godwoken_short_address[2..].as_bytes()).map_err(|err| err.to_string())?,
    );

    let script_hash = godwoken_rpc_client
        .get_script_hash_by_short_address(JsonBytes::from_bytes(short_address))?;

    let script = match script_hash {
        Some(h) => godwoken_rpc_client.get_script(h)?,
        None => return Err("script hash not found!".to_owned()),
    };

    let args = match script {
        Some(s) => s.args,
        None => return Err("script not found!".to_owned()),
    };

    let eth_address = &args.as_bytes()[32..];

    log::info!("eth eoa address: 0x{}", hex::encode(eth_address));

    Ok(())
}
