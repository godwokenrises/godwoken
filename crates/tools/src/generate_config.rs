use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{anyhow, Context, Result};
use ckb_jsonrpc_types::{CellDep, JsonBytes};
use clap::{Parser, ValueEnum};
use gw_builtin_binaries::{file_checksum, Resource};
use gw_config::{
    BackendConfig, BackendForkConfig, BlockProducerConfig, ChainConfig, ChallengerConfig, Config,
    Consensus, ForkConfig, GenesisConfig, NodeMode, P2PNetworkConfig, RPCClientConfig,
    RPCServerConfig, RegistryAddressConfig, RegistryType, SUDTProxyConfig, StoreConfig,
    SystemTypeScriptConfig, WalletConfig,
};
use gw_jsonrpc_types::{godwoken::L2BlockCommittedInfo, JsonCalcHash};
use gw_rpc_client::ckb_client::CkbClient;
use gw_types::prelude::*;
use serde::de::DeserializeOwned;

use crate::{
    deploy_genesis::get_secp_data,
    types::{
        BuildScriptsResult, OmniLockConfig, RollupDeploymentResult, ScriptsDeploymentResult,
        UserRollupConfig,
    },
    utils::cli_args::H160Arg,
};

pub const GENERATE_CONFIG_COMMAND: &str = "generate-config";

#[derive(ValueEnum, Clone)]
enum NodeModeV {
    Readonly,
    Fullnode,
}

impl From<NodeModeV> for NodeMode {
    fn from(m: NodeModeV) -> Self {
        match m {
            NodeModeV::Fullnode => Self::FullNode,
            NodeModeV::Readonly => Self::ReadOnly,
        }
    }
}

/// Generate config
#[derive(Parser)]
#[clap(name = GENERATE_CONFIG_COMMAND)]
pub struct GenerateConfigCommand {
    // Output.
    /// Output config file path
    #[clap(short = 'o', long)]
    output_path: PathBuf,
    /// Output withdrawal to v1 config path
    #[clap(long)]
    output_withdrawal_to_v1_config: Option<PathBuf>,

    // Input.
    /// The scripts deployment results json file path
    #[clap(long)]
    scripts_deployment_path: PathBuf,
    /// The genesis deployment results json file path
    #[clap(short = 'g', long)]
    genesis_deployment_path: PathBuf,
    /// The user rollup config json file path
    #[clap(short, long)]
    rollup_config: PathBuf,
    /// The omni lock config json file path
    #[clap(long)]
    omni_lock_config_path: Option<PathBuf>,
    /// Scripts deployment config json file path
    #[clap(short = 'c', long)]
    scripts_deployment_config_path: PathBuf,

    /// CKB jsonrpc URL
    #[clap(long, default_value = "http://127.0.0.1:8114")]
    ckb_rpc: String,
    /// CKB indexer jsonrpc URL
    #[clap(long)]
    ckb_indexer_rpc: Option<String>,

    #[clap(value_enum, long, default_value_t = NodeModeV::Readonly)]
    node_mode: NodeModeV,
    /// The private key file path
    #[clap(short = 'k', long)]
    privkey_path: Option<PathBuf>,
    /// Store path.
    #[clap(long)]
    store_path: Option<PathBuf>,
    /// Block producer address
    #[clap(long)]
    block_producer_address: Option<H160Arg>,
    /// RPC server listening address
    ///
    /// RPC server listening address in the generated config file.
    #[clap(long, default_value = "localhost:8119")]
    rpc_server_url: String,
    /// P2P network listen multiaddr
    ///
    /// E.g. /ip4/1.2.3.4/tcp/443
    #[clap(long)]
    p2p_listen: Option<String>,
    /// P2P network dial addresses
    ///
    /// E.g. /dns4/godwoken/tcp/443
    #[clap(long)]
    p2p_dial: Vec<String>,
}

impl GenerateConfigCommand {
    pub async fn run(self) -> Result<()> {
        generate_node_config(self).await?;
        Ok(())
    }
}

fn read_json<T: DeserializeOwned>(p: &Path) -> Result<T> {
    let ctx = || format!("read file {}", p.to_string_lossy());
    let c = fs::read(p).with_context(ctx)?;
    let r = serde_json::from_slice(&c).with_context(ctx)?;
    Ok(r)
}

pub async fn generate_node_config(cmd: GenerateConfigCommand) -> Result<()> {
    let rpc_client = CkbClient::with_url(&cmd.ckb_rpc)?;
    let scripts: BuildScriptsResult = read_json(&cmd.scripts_deployment_config_path)?;
    let scripts_deployment: ScriptsDeploymentResult = read_json(&cmd.scripts_deployment_path)?;
    let rollup_result: RollupDeploymentResult = read_json(&cmd.genesis_deployment_path)?;
    let user_rollup_config: UserRollupConfig = read_json(&cmd.rollup_config)?;
    let omni_lock_config: Option<OmniLockConfig> = if let Some(ref o) = cmd.omni_lock_config_path {
        Some(read_json(o)?)
    } else {
        None
    };

    let tx_with_status = rpc_client
        .get_transaction(rollup_result.tx_hash.clone(), 2.into())
        .await?
        .context("can't find genesis block transaction")?;
    let block_hash = tx_with_status.tx_status.block_hash.ok_or_else(|| {
        anyhow!(
            "the genesis transaction haven't been packaged into chain, please retry after a while"
        )
    })?;
    let number = rpc_client
        .get_header(block_hash.clone())
        .await?
        .ok_or_else(|| anyhow!("can't find block"))?
        .inner
        .number;

    // build configuration
    let rollup_config = rollup_result.rollup_config.clone();
    let rollup_type_hash = rollup_result.rollup_type_hash.clone();
    let meta_contract_validator_type_hash = scripts_deployment
        .meta_contract_validator
        .script_type_hash
        .clone();
    let eth_registry_validator_type_hash = scripts_deployment
        .eth_addr_reg_validator
        .script_type_hash
        .clone();
    let rollup_type_script = rollup_result.rollup_type_script.clone();
    let rollup_config_cell_dep = rollup_result.rollup_config_cell_dep.clone();
    let (_data, secp_data_dep) = get_secp_data(&rpc_client).await.context("get secp data")?;

    let system_type_scripts = query_contracts_script(
        &rpc_client,
        &scripts_deployment,
        &user_rollup_config,
        &omni_lock_config,
        rollup_result.delegate_cell_type_script.clone(),
    )
    .await
    .map_err(|err| anyhow!("query contracts script {}", err))?;

    let challenger_config = ChallengerConfig {
        rewards_receiver_lock: user_rollup_config.reward_lock.clone(),
    };

    let wallet_config = cmd.privkey_path.map(|p| WalletConfig { privkey_path: p });

    let backends: Vec<BackendConfig> = vec![
        {
            let generator_path = scripts.built_scripts["meta_contract_generator"].clone();
            let generator = Resource::file_system(generator_path.clone());
            let generator_checksum = file_checksum(&generator_path)?.into();
            BackendConfig {
                generator,
                generator_checksum,
                validator_script_type_hash: scripts_deployment
                    .meta_contract_validator
                    .script_type_hash
                    .clone(),
                backend_type: gw_config::BackendType::Meta,
            }
        },
        {
            let generator_path = scripts.built_scripts["l2_sudt_generator"].clone();
            let generator = Resource::file_system(generator_path.clone());
            let generator_checksum = file_checksum(&generator_path)?.into();
            BackendConfig {
                generator,
                generator_checksum,
                validator_script_type_hash: scripts_deployment
                    .l2_sudt_validator
                    .script_type_hash
                    .clone(),
                backend_type: gw_config::BackendType::Sudt,
            }
        },
        {
            let generator_path = scripts.built_scripts["polyjuice_generator"].clone();
            let generator = Resource::file_system(generator_path.clone());
            let generator_checksum = file_checksum(&generator_path)?.into();
            BackendConfig {
                generator,
                generator_checksum,
                validator_script_type_hash: scripts_deployment
                    .polyjuice_validator
                    .script_type_hash
                    .clone(),
                backend_type: gw_config::BackendType::Polyjuice,
            }
        },
        {
            let generator_path = scripts.built_scripts["eth_addr_reg_generator"].clone();
            let generator = Resource::file_system(generator_path.clone());
            let generator_checksum = file_checksum(&generator_path)?.into();
            BackendConfig {
                generator,
                generator_checksum,
                validator_script_type_hash: scripts_deployment
                    .eth_addr_reg_validator
                    .script_type_hash
                    .clone(),
                backend_type: gw_config::BackendType::EthAddrReg,
            }
        },
    ];
    let backend_forks = vec![BackendForkConfig {
        fork_height: 0,
        sudt_proxy: Some(SUDTProxyConfig {
            permit_sudt_transfer_from_dangerous_contract: true,
            address_list: Vec::new(),
        }),
        backends,
    }];

    let genesis_committed_info = L2BlockCommittedInfo {
        block_hash,
        number,
        transaction_hash: rollup_result.tx_hash.clone(),
    };

    let chain: ChainConfig = ChainConfig {
        genesis_committed_info,
        rollup_type_script,
        skipped_invalid_block_list: Default::default(),
        burn_lock: user_rollup_config.burn_lock,
        // cell deps
        rollup_config_cell_dep,
    };

    let genesis: GenesisConfig = GenesisConfig {
        timestamp: rollup_result.timestamp,
        rollup_type_hash: rollup_type_hash.clone(),
        meta_contract_validator_type_hash,
        eth_registry_validator_type_hash,
        rollup_config,
        secp_data_dep,
    };

    let fork = ForkConfig {
        backend_forks,
        increase_max_l2_tx_cycles_to_500m: None,
        upgrade_global_state_version_to_v2: Some(0),
        genesis,
        chain,
        system_type_scripts,
        pending_l1_upgrades: Default::default(),
    };

    let store = StoreConfig {
        path: cmd.store_path.unwrap_or_else(|| "./gw-db".into()),
        options_file: None,
        cache_size: None,
    };
    let rpc_client: RPCClientConfig = RPCClientConfig {
        indexer_url: cmd.ckb_indexer_rpc,
        ckb_url: cmd.ckb_rpc,
    };
    let rpc_server = RPCServerConfig {
        listen: cmd.rpc_server_url,
        ..Default::default()
    };
    let block_producer = Some(BlockProducerConfig {
        block_producer: RegistryAddressConfig {
            address_type: RegistryType::Eth,
            address: JsonBytes::from_vec(
                cmd.block_producer_address
                    .unwrap_or_default()
                    .0
                    .as_bytes()
                    .to_vec(),
            ),
        },
        challenger_config,
        wallet_config,
        ..Default::default()
    });
    let p2p_network_config = if !cmd.p2p_dial.is_empty() || cmd.p2p_listen.is_some() {
        Some(P2PNetworkConfig {
            listen: cmd.p2p_listen,
            dial: cmd.p2p_dial,
            ..Default::default()
        })
    } else {
        None
    };

    let config: Config = Config {
        consensus: Consensus::Config {
            config: Box::new(fork),
        },
        rpc_client,
        rpc_server,
        block_producer,
        node_mode: cmd.node_mode.into(),
        store,
        p2p_network_config,
        ..Default::default()
    };

    if let Some(p) = cmd.output_path.parent() {
        fs::create_dir_all(p)?;
    }
    fs::write(cmd.output_path, toml::to_string_pretty(&config)?)?;
    if let Some(w) = cmd.output_withdrawal_to_v1_config {
        let rollup_type_hash = format!("0x{rollup_type_hash}");
        let deposit_lock_code_hash =
            format!("0x{}", scripts_deployment.deposit_lock.script_type_hash);
        let eth_lock_code_hash =
            format!("0x{}", scripts_deployment.eth_account_lock.script_type_hash);
        if let Some(p) = w.parent() {
            fs::create_dir_all(p)?;
        }
        fs::write(
            w,
            toml::to_string_pretty(&toml::toml! {
                [withdrawal_to_v1_config]
                v1_rollup_type_hash = rollup_type_hash
                v1_deposit_lock_code_hash = deposit_lock_code_hash
                v1_eth_lock_code_hash = eth_lock_code_hash
                v1_deposit_minimal_cancel_timeout_msecs = 604800000
            })?,
        )?;
    }

    Ok(())
}

async fn query_contracts_script(
    ckb_client: &CkbClient,
    deployment: &ScriptsDeploymentResult,
    user_rollup_config: &UserRollupConfig,
    omni_lock_config: &Option<OmniLockConfig>,
    delegate_cell_type_script: gw_jsonrpc_types::blockchain::Script,
) -> Result<SystemTypeScriptConfig> {
    let query = |contract: &'static str, cell_dep: CellDep| -> _ {
        ckb_client.query_type_script(contract, cell_dep)
    };

    let l1_sudt = query("l1 sudt", user_rollup_config.l1_sudt_cell_dep.clone()).await?;
    assert_eq!(l1_sudt.hash(), user_rollup_config.l1_sudt_script_type_hash);

    let d = deployment;
    let allowed_eoa_scripts = vec![d.eth_account_lock.type_script.clone()];

    let allowed_contract_scripts = vec![
        d.meta_contract_validator.type_script.clone(),
        d.l2_sudt_validator.type_script.clone(),
        d.polyjuice_validator.type_script.clone(),
        d.eth_addr_reg_validator.type_script.clone(),
    ];

    let omni_lock = if let Some(o) = omni_lock_config {
        let omni_lock = query("omni lock", o.cell_dep.clone()).await?;
        assert_eq!(omni_lock.hash(), o.script_type_hash);
        omni_lock
    } else {
        d.omni_lock.type_script.clone()
    };

    Ok(SystemTypeScriptConfig {
        state_validator: d.state_validator.type_script.clone(),
        deposit_lock: d.deposit_lock.type_script.clone(),
        stake_lock: d.stake_lock.type_script.clone(),
        custodian_lock: d.custodian_lock.type_script.clone(),
        withdrawal_lock: d.withdrawal_lock.type_script.clone(),
        challenge_lock: d.challenge_lock.type_script.clone(),
        l1_sudt,
        omni_lock,
        allowed_eoa_scripts,
        allowed_contract_scripts,
        delegate_cell_lock: Some(d.delegate_cell_lock.type_script.clone()),
        delegate_cell: Some(delegate_cell_type_script),
    })
}
