use crate::deploy_genesis::deploy_genesis;
use crate::deploy_scripts::deploy_scripts;
use crate::generate_config::generate_config;
use crate::prepare_scripts::{self, prepare_scripts, ScriptsBuildMode};
use crate::utils;
use crate::utils::transaction::run_in_output_mode;
use ckb_sdk::Address;
use ckb_types::{
    core::ScriptHashType, packed as ckb_packed, prelude::Builder as CKBBuilder,
    prelude::Pack as CKBPack, prelude::Unpack as CKBUnpack,
};
use gw_types::prelude::Entity as GwEntity;
use rand::Rng;
use serde::Serialize;
use serde_json::json;
use std::fs;
use std::str::FromStr;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    thread, time,
};

#[derive(Debug)]
pub struct NodeWalletInfo {
    pub testnet_address: String,
    pub lock_hash: String,
    pub lock_arg: String,
    pub block_assembler_code_hash: String,
}

#[allow(clippy::too_many_arguments)]
pub fn setup(
    ckb_rpc_url: &str,
    indexer_url: &str,
    mode: ScriptsBuildMode,
    scripts_path: &Path,
    privkey_path: &Path,
    cells_lock_address: &str,
    nodes_count: usize,
    server_url: &str,
    output_dir: &Path,
    transfer_capacity: u64,
) {
    let mut prepare_scripts_result_path = output_dir.join("scripts-deploy.json");
    prepare_scripts(
        mode,
        scripts_path,
        Path::new(prepare_scripts::REPOS_DIR_PATH),
        Path::new(prepare_scripts::SCRIPTS_DIR_PATH),
        &prepare_scripts_result_path,
    )
    .expect("prepare scripts");

    let scripts_deployment_result = output_dir.join("scripts-deploy-result.json");
    let cells_lock: ckb_types::packed::Script = Address::from_str(cells_lock_address)
        .unwrap()
        .payload()
        .into();
    deploy_scripts(
        privkey_path,
        ckb_rpc_url,
        &prepare_scripts_result_path,
        &scripts_deployment_result,
        Some(cells_lock.into()),
    )
    .expect("deploy scripts");

    let poa_config_path = output_dir.join("poa-config.json");
    let rollup_config_path = output_dir.join("rollup-config.json");
    prepare_nodes_configs(
        privkey_path,
        transfer_capacity,
        nodes_count,
        output_dir,
        &poa_config_path,
        &rollup_config_path,
    );

    let genesis_deploy_result = output_dir.join("genesis-deploy-result.json");
    deploy_genesis(
        privkey_path,
        ckb_rpc_url,
        &scripts_deployment_result,
        &rollup_config_path,
        &poa_config_path,
        None,
        &genesis_deploy_result,
        false,
    )
    .expect("deploy genesis");

    (0..nodes_count).for_each(|index| {
        let node_name = format!("node{}", index + 1);
        let privkey_path = output_dir.join(&node_name).join("pk");
        let output_file_path = output_dir.join(node_name).join("config.toml");
        generate_config(
            &genesis_deploy_result,
            &scripts_deployment_result,
            privkey_path.as_ref(),
            ckb_rpc_url.to_owned(),
            indexer_url.to_owned(),
            output_file_path.as_ref(),
            None,
            &prepare_scripts_result_path,
            server_url.to_string(),
        )
        .expect("generate_config");
    });

    log::info!("Finish");
}

fn prepare_nodes_configs(
    payer_privkey: &Path,
    capacity: u64,
    nodes_count: usize,
    output_dir: &Path,
    poa_config_path: &Path,
    rollup_config_path: &Path,
) {
    let nodes_privkeys = prepare_privkeys(output_dir, nodes_count);
    let nodes_info = check_wallets_info(nodes_privkeys, capacity, payer_privkey);
    generate_poa_config(&nodes_info, poa_config_path);
    generate_rollup_config(rollup_config_path);
}

fn prepare_privkeys(output_dir: &Path, nodes_count: usize) -> HashMap<String, PathBuf> {
    (0..nodes_count)
        .map(|index| {
            let node_name = format!("node{}", (index + 1).to_string());
            let node_dir = output_dir.join(node_name);
            fs::create_dir_all(&node_dir).expect("create node dir");
            let privkey_file = node_dir.join("pk");
            let privkey = fs::read_to_string(&privkey_file)
                .map(|s| s.trim().into())
                .unwrap_or_else(|_| Vec::new());
            if !privkey.starts_with(b"0x")
                || privkey.len() != 66
                || hex::decode(&privkey[2..]).is_err()
            {
                log::info!("Generate privkey file...");
                generate_privkey_file(&privkey_file);
            }
            (node_name, privkey_file)
        })
        .collect()
}

fn check_wallets_info(
    nodes_privkeys: HashMap<String, PathBuf>,
    capacity: u64,
    payer_privkey_path: &Path,
) -> HashMap<String, NodeWalletInfo> {
    nodes_privkeys
        .into_iter()
        .map(|(node, privkey)| {
            let wallet_info = get_wallet_info(&privkey);
            let mut current_capacity = query_wallet_capacity(&wallet_info.testnet_address);
            log::info!("{}'s wallet capacity: {}", node, current_capacity);
            log::info!("Start to transfer ckb, and it will take 30 seconds...");
            transfer_ckb(&wallet_info, payer_privkey_path, capacity);
            thread::sleep(time::Duration::from_secs(30));
            current_capacity = query_wallet_capacity(&wallet_info.testnet_address);
            log::info!("{}'s wallet capacity: {}", node, current_capacity);
            (node, wallet_info)
        })
        .collect()
}

fn generate_poa_config(nodes_info: &HashMap<String, NodeWalletInfo>, poa_config_path: &Path) {
    let identities: Vec<&str> = nodes_info
        .iter()
        .map(|(_, node)| node.lock_hash.as_str())
        .collect();
    let poa_config = json!({
        "poa_setup" : {
            "identity_size": 32,
            "round_interval_uses_seconds": true,
            "identities": identities,
            "aggregator_change_threshold": identities.len(),
            "round_intervals": 24,
            "subblocks_per_round": 1
        }
    });
    generate_json_file(&poa_config, poa_config_path);
}

fn generate_rollup_config(rollup_config_path: &Path) {
    let burn_lock_script = ckb_packed::Script::new_builder()
        .code_hash(CKBPack::pack(&[0u8; 32]))
        .hash_type(ScriptHashType::Data.into())
        .build();
    let burn_lock_script_hash: [u8; 32] = burn_lock_script.calc_script_hash().unpack();
    let rollup_config = json!({
      "l1_sudt_script_type_hash": "0x0000000000000000000000000000000000000000000000000000000000000000",
      "burn_lock_hash": format!("0x{}", hex::encode(burn_lock_script_hash)),
      "required_staking_capacity": 10000000000u64,
      "challenge_maturity_blocks": 5,
      "finality_blocks": 20,
      "reward_burn_rate": 50,
      "compatible_chain_id": 0,
      "allowed_eoa_type_hashes": []
    });
    generate_json_file(&rollup_config, rollup_config_path);
    log::info!("Finish");
}

fn generate_privkey_file(privkey_file_path: &Path) {
    let key = rand::thread_rng().gen::<[u8; 32]>();
    let privkey = format!("0x{}", hex::encode(key));
    fs::write(&privkey_file_path, &privkey).expect("create pk file");
}

pub fn get_wallet_info(privkey_path: &Path) -> NodeWalletInfo {
    let (stdout, stderr) = run_in_output_mode(
        "ckb-cli",
        vec![
            "util",
            "key-info",
            "--privkey-path",
            &privkey_path.display().to_string(),
        ],
    )
    .expect("get key info");
    NodeWalletInfo {
        testnet_address: look_after_in_line(&stdout, "testnet:"),
        lock_hash: look_after_in_line(&stdout, "lock_hash:"),
        lock_arg: look_after_in_line(&stdout, "lock_arg:"),
        block_assembler_code_hash: look_after_in_line(&stderr, "code_hash ="),
    }
}

fn query_wallet_capacity(address: &str) -> f64 {
    let (stdout, _) = run_in_output_mode(
        "ckb-cli",
        vec!["wallet", "get-capacity", "--address", address],
    )
    .expect("query wallet capacity");
    look_after_in_line(&stdout, "total:")
        .split(' ')
        .collect::<Vec<&str>>()[0]
        .parse::<f64>()
        .expect("parse capacity")
}

fn transfer_ckb(node_wallet: &NodeWalletInfo, payer_privkey_path: &Path, capacity: u64) {
    utils::transaction::run(
        "ckb-cli",
        vec![
            "wallet",
            "transfer",
            "--to-address",
            &node_wallet.testnet_address,
            "--capacity",
            &capacity.to_string(),
            "--tx-fee",
            "1",
            "--privkey-path",
            &payer_privkey_path.display().to_string(),
        ],
    )
    .expect("transfer ckb");
}

fn look_after_in_line(text: &str, key: &str) -> String {
    text.split(key).collect::<Vec<&str>>()[1]
        .split('\n')
        .collect::<Vec<&str>>()[0]
        .trim_matches(&['"', ' '][..])
        .to_owned()
}

fn generate_json_file<T>(value: &T, json_file_path: &Path)
where
    T: Serialize,
{
    let output_content = serde_json::to_string_pretty(value).expect("serde json to string pretty");
    let output_dir = json_file_path.parent().expect("get output dir");
    fs::create_dir_all(&output_dir).expect("create output dir");
    fs::write(json_file_path, output_content.as_bytes()).expect("generate json file");
}
