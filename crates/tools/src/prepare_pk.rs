use crate::utils;
use rand::Rng;
use serde::Serialize;
use serde_json::json;
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    thread, time,
};

const MIN_WALLET_CAPACITY: f64 = 100000.0f64;

#[derive(Debug)]
struct NodeWalletInfo {
    node_name: String,
    privkey_path: PathBuf,
    testnet_address: String,
    lock_hash: String,
    lock_arg: String,
    block_assembler_code_hash: String,
}

pub fn prepare_pk(
    payer_privkey: &Path,
    ckb_count: u32,
    nodes_count: u8,
    output_dir: &Path,
    poa_config_path: &Path,
    rollup_config_path: &Path,
) {
    let nodes_privkeys = prepare_privkeys(output_dir, nodes_count);
    let nodes_info = check_wallets_info(nodes_privkeys, ckb_count, payer_privkey);
    generate_poa_config(&nodes_info, poa_config_path);
    generate_rollup_config(rollup_config_path);
}

fn prepare_privkeys(output_dir: &Path, nodes_count: u8) -> HashMap<String, PathBuf> {
    (0..nodes_count)
        .map(|index| {
            let node_name = format!("node{}", (index + 1).to_string());
            let node_dir = utils::make_path(output_dir, vec![&node_name]);
            fs::create_dir_all(&node_dir).expect("create node dir");
            let privkey_file = utils::make_path(&node_dir, vec!["pk"]);
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
    ckb_count: u32,
    payer_privkey_path: &Path,
) -> Vec<NodeWalletInfo> {
    nodes_privkeys
        .into_iter()
        .map(|(node, privkey)| {
            let wallet_info = get_wallet_info(&node, privkey);
            let mut capacity = query_wallet_capacity(&wallet_info.testnet_address);
            log::info!("{}'s wallet capacity: {}", node, capacity);
            if capacity < MIN_WALLET_CAPACITY {
                log::info!("Start to transfer ckb, and it will take 30 seconds...");
                transfer_ckb(&wallet_info, payer_privkey_path, ckb_count);
                thread::sleep(time::Duration::from_secs(30));
                capacity = query_wallet_capacity(&wallet_info.testnet_address);
                assert!(
                    capacity >= MIN_WALLET_CAPACITY,
                    "wallet haven't received ckb, please try again"
                );
                log::info!("{}'s wallet capacity: {}", node, capacity);
            }
            wallet_info
        })
        .collect()
}

fn generate_poa_config(nodes_info: &[NodeWalletInfo], poa_config_path: &Path) {
    let identities: Vec<&str> = nodes_info
        .iter()
        .map(|node| node.lock_hash.as_str())
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
    let required_staking_capacity = 10000000000u64;
    let rollup_config = json!({
      "l1_sudt_script_type_hash": "0x0000000000000000000000000000000000000000000000000000000000000000",
      "burn_lock_hash": "0x0000000000000000000000000000000000000000000000000000000000000000",
      "required_staking_capacity": required_staking_capacity,
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

fn get_wallet_info(node_name: &str, privkey_path: PathBuf) -> NodeWalletInfo {
    let (stdout, stderr) = utils::run_in_output_mode(
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
        node_name: node_name.into(),
        privkey_path,
        testnet_address: look_after_in_line(&stdout, "testnet:"),
        lock_hash: look_after_in_line(&stdout, "lock_hash:"),
        lock_arg: look_after_in_line(&stdout, "lock_arg:"),
        block_assembler_code_hash: look_after_in_line(&stderr, "code_hash ="),
    }
}

fn query_wallet_capacity(address: &str) -> f64 {
    let (stdout, _) = utils::run_in_output_mode(
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

fn transfer_ckb(node_wallet: &NodeWalletInfo, payer_privkey_path: &Path, ckb_count: u32) {
    utils::run(
        "ckb-cli",
        vec![
            "wallet",
            "transfer",
            "--to-address",
            &node_wallet.testnet_address,
            "--capacity",
            &ckb_count.to_string(),
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
