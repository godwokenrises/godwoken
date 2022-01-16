use crate::deploy_genesis::{deploy_rollup_cell, DeployRollupCellArgs};
use crate::deploy_scripts::deploy_scripts;
use crate::generate_config::{generate_node_config, GenerateNodeConfigArgs};
use crate::prepare_scripts::{self, prepare_scripts, ScriptsBuildMode};
use crate::types::{PoAConfig, PoASetup, SetupConfig, UserRollupConfig};
use crate::utils;
use crate::utils::transaction::run_in_output_mode;
use anyhow::Result;
use clap::arg_enum;
use gw_config::NodeMode;
use rand::Rng;
use std::fs;
use std::{
    path::{Path, PathBuf},
    thread, time,
};

arg_enum! {
    #[derive(Debug, Clone, Copy)]
    pub enum WalletNetwork {
        Testnet,
        Mainnet,
        Devnet,
    }
}
#[derive(Debug)]
pub struct NodeWalletInfo {
    pub testnet_address: String,
    pub mainnet_address: String,
    pub lock_hash: String,
    pub lock_arg: String,
    pub block_assembler_code_hash: String,
}

impl NodeWalletInfo {
    fn address(&self, network: WalletNetwork) -> &String {
        match network {
            WalletNetwork::Mainnet => &self.mainnet_address,
            WalletNetwork::Testnet => &self.testnet_address,
            WalletNetwork::Devnet => &self.testnet_address,
        }
    }
}

pub struct SetupArgs<'a> {
    pub ckb_rpc_url: &'a str,
    pub indexer_url: &'a str,
    pub mode: ScriptsBuildMode,
    pub build_scripts_config_path: &'a Path,
    pub privkey_path: &'a Path,
    pub nodes_count: usize,
    pub server_url: &'a str,
    pub output_dir: &'a Path,
    pub setup_config_path: &'a Path,
    pub wallet_network: WalletNetwork,
}

pub async fn setup(args: SetupArgs<'_>) {
    let SetupArgs {
        ckb_rpc_url,
        indexer_url,
        mode,
        build_scripts_config_path,
        privkey_path,
        nodes_count,
        server_url,
        output_dir,
        setup_config_path,
        wallet_network,
    } = args;

    let setup_config: SetupConfig = {
        let content = fs::read(setup_config_path).unwrap();
        serde_json::from_slice(&content).unwrap()
    };

    // prepare scripts
    let build_scripts_result = {
        let output_path = output_dir.join("scripts-config.json");
        log::info!("Generate {:?} ...", &output_path);
        let build_scripts_result = prepare_scripts(
            mode,
            setup_config.cells_lock.clone(),
            build_scripts_config_path,
            &output_dir.join(prepare_scripts::SCRIPT_BUILD_DIR_PATH),
            &output_dir.join(prepare_scripts::SCRIPTS_DIR_PATH),
        )
        .expect("prepare scripts");
        let output_content = serde_json::to_string_pretty(&build_scripts_result)
            .expect("serde json to string pretty");
        let output_dir = output_path.parent().expect("get output dir");
        fs::create_dir_all(&output_dir).expect("create output dir");
        fs::write(output_path, output_content.as_bytes()).expect("output config");
        build_scripts_result
    };
    log::info!("Done");

    // deploy scripts
    let deploy_scripts_result = {
        let scripts_deploy_result = output_dir.join("scripts-result.json");
        let deploy_result = deploy_scripts(privkey_path, ckb_rpc_url, &build_scripts_result)
            .expect("deploy scripts");
        let output_content =
            serde_json::to_string_pretty(&deploy_result).expect("serde json to string pretty");
        fs::write(scripts_deploy_result, output_content.as_bytes())
            .map_err(|err| err.to_string())
            .unwrap();
        deploy_result
    };

    // setup nodes
    let nodes = setup_nodes(
        privkey_path,
        setup_config.node_initial_ckb,
        nodes_count,
        output_dir,
        wallet_network,
    );

    // setup POA config
    let poa_config = {
        let poa_config_path = output_dir.join("poa-config.json");
        let poa_config = generate_poa_config(&nodes);
        let output_content =
            serde_json::to_string_pretty(&poa_config).expect("serde json to string pretty");
        fs::write(poa_config_path, output_content.as_bytes()).expect("output config");
        poa_config
    };

    // setup rollup config
    let rollup_config = {
        let rollup_config_path = output_dir.join("rollup-config.json");
        let rollup_config = generate_rollup_config(&setup_config).unwrap();
        let output_content =
            serde_json::to_string_pretty(&rollup_config).expect("serde json to string pretty");
        fs::write(rollup_config_path, output_content.as_bytes())
            .map_err(|err| err.to_string())
            .unwrap();
        rollup_config
    };

    // deploy rollup cell
    let rollup_result = {
        let rollup_result_path = output_dir.join("rollup-result.json");
        let args = DeployRollupCellArgs {
            privkey_path,
            ckb_rpc_url,
            scripts_result: &deploy_scripts_result,
            user_rollup_config: &rollup_config,
            poa_config: &poa_config,
            timestamp: None,
            skip_config_check: false,
        };
        let rollup_result = deploy_rollup_cell(args).expect("deploy rollup cell");
        let output_content =
            serde_json::to_string_pretty(&rollup_result).expect("serde json to string pretty");
        fs::write(rollup_result_path, output_content.as_bytes())
            .map_err(|err| err.to_string())
            .unwrap();
        rollup_result
    };

    // generate node config
    for (index, (node_name, _node_wallet)) in nodes.iter().enumerate() {
        let privkey_path = output_dir.join(&node_name).join("pk");
        let output_file_path = output_dir.join(node_name).join("config.toml");
        // set the first node to fullnode
        let node_mode = if index == 0 {
            NodeMode::FullNode
        } else {
            NodeMode::ReadOnly
        };
        let args = GenerateNodeConfigArgs {
            rollup_result: &rollup_result,
            scripts_deployment: &deploy_scripts_result,
            privkey_path: &privkey_path,
            ckb_url: ckb_rpc_url.to_string(),
            indexer_url: indexer_url.to_string(),
            database_url: None,
            build_scripts_result: &build_scripts_result,
            server_url: server_url.to_string(),
            user_rollup_config: &rollup_config,
            node_mode,
        };
        let config = generate_node_config(args).await.expect("generate_config");
        let output_content = toml::to_string_pretty(&config).expect("serde toml to string pretty");
        fs::write(output_file_path, output_content.as_bytes()).unwrap();
    }

    log::info!("Finish");
}

fn setup_nodes(
    payer_privkey: &Path,
    node_initial_ckb: u64,
    nodes_count: usize,
    output_dir: &Path,
    network: WalletNetwork,
) -> Vec<(String, NodeWalletInfo)> {
    (0..nodes_count)
        .map(|i| {
            let node_name = format!("node{}", (i + 1).to_string());
            let node_dir = output_dir.join(&node_name);
            log::info!("Generate privkey file for {}...", &node_name);
            let node_pk_path = prepare_privkey(&node_dir);
            log::info!("Initialize wallet for {}...", &node_name);
            let node_wallet =
                init_node_wallet(&node_pk_path, node_initial_ckb, payer_privkey, network);
            (node_name, node_wallet)
        })
        .collect()
}

fn prepare_privkey(node_dir: &Path) -> PathBuf {
    fs::create_dir_all(&node_dir).expect("create node dir");
    let privkey_file = node_dir.join("pk");
    generate_privkey_file(&privkey_file);
    privkey_file
}

fn init_node_wallet(
    node_privkey: &Path,
    node_initial_ckb: u64,
    payer_privkey_path: &Path,
    network: WalletNetwork,
) -> NodeWalletInfo {
    let wallet_info = get_wallet_info(node_privkey);
    let mut current_capacity = query_wallet_capacity(wallet_info.address(network));
    log::info!("node's wallet capacity: {}", current_capacity);
    log::info!("Start to transfer ckb, and it will take 30 seconds...");
    transfer_ckb(&wallet_info, payer_privkey_path, node_initial_ckb, network);
    loop {
        thread::sleep(time::Duration::from_secs(5));
        current_capacity = query_wallet_capacity(wallet_info.address(network));
        if current_capacity > 0f64 {
            break;
        }
    }
    log::info!("node's wallet capacity: {}", current_capacity);
    wallet_info
}

fn generate_poa_config(nodes: &[(String, NodeWalletInfo)]) -> PoAConfig {
    let identities: Vec<_> = nodes
        .iter()
        .map(|(_, node)| {
            let lock_hash = hex::decode(&node.lock_hash.trim_start_matches("0x")).unwrap();
            ckb_jsonrpc_types::JsonBytes::from_vec(lock_hash)
        })
        .collect();
    PoAConfig {
        poa_setup: PoASetup {
            identity_size: 32,
            round_interval_uses_seconds: true,
            aggregator_change_threshold: identities.len() as u8,
            identities,
            round_intervals: 24,
            subblocks_per_round: 1,
        },
    }
}

fn generate_rollup_config(setup_config: &SetupConfig) -> Result<UserRollupConfig> {
    let rollup_config = UserRollupConfig {
        l1_sudt_script_type_hash: setup_config.l1_sudt_script_type_hash.clone(),
        l1_sudt_cell_dep: setup_config.l1_sudt_cell_dep.clone(),
        burn_lock: setup_config.burn_lock.clone(),
        reward_lock: setup_config.reward_lock.clone(),
        required_staking_capacity: 10000000000u64,
        challenge_maturity_blocks: 450,
        finality_blocks: 3600,
        reward_burn_rate: 50,
        allowed_eoa_type_hashes: Vec::new(),
        cells_lock: setup_config.cells_lock.clone(),
    };
    Ok(rollup_config)
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
        mainnet_address: look_after_in_line(&stdout, "mainnet:"),
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

fn transfer_ckb(
    node_wallet: &NodeWalletInfo,
    payer_privkey_path: &Path,
    ckb_amount: u64,
    network: WalletNetwork,
) {
    utils::transaction::run(
        "ckb-cli",
        vec![
            "wallet",
            "transfer",
            "--to-address",
            node_wallet.address(network),
            "--capacity",
            &ckb_amount.to_string(),
            "--tx-fee",
            "0.1",
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
