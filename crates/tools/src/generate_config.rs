use crate::deploy_genesis::{get_secp_data, GenesisDeploymentResult};
use crate::deploy_scripts::ScriptsDeploymentResult;
use crate::setup::get_wallet_info;
use anyhow::{anyhow, Result};
use ckb_sdk::HttpRpcClient;
use ckb_types::prelude::{Builder, Entity};
use gw_config::{
    BackendConfig, BlockProducerConfig, ChainConfig, ChallengerConfig, Config, GenesisConfig,
    NodeMode, RPCClientConfig, RPCServerConfig, StoreConfig, WalletConfig, Web3IndexerConfig,
};
use gw_jsonrpc_types::godwoken::L2BlockCommittedInfo;
use gw_types::{core::ScriptHashType, packed::Script, prelude::*};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Debug)]
struct ScriptsBuilt {
    built_scripts: HashMap<String, PathBuf>,
}

impl ScriptsBuilt {
    fn get_path(&self, name: &str) -> PathBuf {
        self.built_scripts
            .get(name)
            .expect("get script path")
            .into()
    }
}

#[allow(clippy::too_many_arguments)]
pub fn generate_config(
    genesis_path: &Path,
    scripts_results_path: &Path,
    privkey_path: &Path,
    ckb_url: String,
    indexer_url: String,
    output_path: &Path,
    database_url: Option<&str>,
    scripts_config_path: &Path,
    server_url: String,
) -> Result<()> {
    let genesis: GenesisDeploymentResult = {
        let content = fs::read(genesis_path)?;
        serde_json::from_slice(&content)?
    };
    let scripts_results: ScriptsDeploymentResult = {
        let content = fs::read(scripts_results_path)?;
        serde_json::from_slice(&content)?
    };
    let scripts_built: ScriptsBuilt = {
        let content = fs::read(scripts_config_path)?;
        serde_json::from_slice(&content)?
    };

    let mut rpc_client = HttpRpcClient::new(ckb_url.to_string());
    let tx_with_status = rpc_client
        .get_transaction(genesis.tx_hash.clone())
        .map_err(|err| anyhow!("{}", err))?
        .ok_or_else(|| anyhow!("can't find genesis block transaction"))?;
    let block_hash = tx_with_status.tx_status.block_hash.ok_or_else(|| {
        anyhow!(
            "the genesis transaction haven't been packaged into chain, please retry after a while"
        )
    })?;
    let number = rpc_client
        .get_header(block_hash.clone())
        .map_err(|err| anyhow!("{}", err))?
        .ok_or_else(|| anyhow!("can't find block"))?
        .inner
        .number
        .into();

    // build configuration
    let account_id = 0;
    let node_wallet_info = get_wallet_info(privkey_path);
    let code_hash: [u8; 32] = {
        let mut hash = [0u8; 32];
        hex::decode_to_slice(
            node_wallet_info
                .block_assembler_code_hash
                .strip_prefix("0x")
                .expect("get code hash"),
            &mut hash as &mut [u8],
        )?;
        hash
    };
    let args = hex::decode(
        node_wallet_info
            .lock_arg
            .strip_prefix("0x")
            .expect("get args"),
    )?;
    let lock = Script::new_builder()
        .code_hash(code_hash.pack())
        .hash_type(ScriptHashType::Type.into())
        .args(args.pack())
        .build()
        .into();

    let rollup_config = genesis.rollup_config.clone();
    let rollup_type_hash = genesis.rollup_type_hash;
    let meta_contract_validator_type_hash = scripts_results
        .meta_contract_validator
        .script_type_hash
        .clone();
    let rollup_type_script = {
        let script: ckb_types::packed::Script = genesis.rollup_type_script.into();
        gw_types::packed::Script::new_unchecked(script.as_bytes()).into()
    };
    let rollup_config_cell_dep = {
        let cell_dep: ckb_types::packed::CellDep = genesis.rollup_config_cell_dep.into();
        gw_types::packed::CellDep::new_unchecked(cell_dep.as_bytes()).into()
    };
    let poa_lock_dep = {
        let dep: ckb_types::packed::CellDep =
            scripts_results.state_validator_lock.cell_dep.clone().into();
        gw_types::packed::CellDep::new_unchecked(dep.as_bytes()).into()
    };
    let poa_state_dep = {
        let dep: ckb_types::packed::CellDep = scripts_results.poa_state.cell_dep.clone().into();
        gw_types::packed::CellDep::new_unchecked(dep.as_bytes()).into()
    };
    let rollup_cell_type_dep = {
        let dep: ckb_types::packed::CellDep =
            scripts_results.state_validator.cell_dep.clone().into();
        gw_types::packed::CellDep::new_unchecked(dep.as_bytes()).into()
    };
    let deposit_cell_lock_dep = {
        let dep: ckb_types::packed::CellDep = scripts_results.deposit_lock.cell_dep.clone().into();
        gw_types::packed::CellDep::new_unchecked(dep.as_bytes()).into()
    };
    let stake_cell_lock_dep = {
        let dep: ckb_types::packed::CellDep = scripts_results.stake_lock.cell_dep.clone().into();
        gw_types::packed::CellDep::new_unchecked(dep.as_bytes()).into()
    };
    let (_data, secp_data_dep) =
        get_secp_data(&mut rpc_client).map_err(|err| anyhow!("{}", err))?;
    let custodian_cell_lock_dep = {
        let dep: ckb_types::packed::CellDep =
            scripts_results.custodian_lock.cell_dep.clone().into();
        gw_types::packed::CellDep::new_unchecked(dep.as_bytes()).into()
    };
    let withdrawal_cell_lock_dep = {
        let dep: ckb_types::packed::CellDep =
            scripts_results.withdrawal_lock.cell_dep.clone().into();
        gw_types::packed::CellDep::new_unchecked(dep.as_bytes()).into()
    };
    let challenge_cell_lock_dep = {
        let dep: ckb_types::packed::CellDep =
            scripts_results.challenge_lock.cell_dep.clone().into();
        gw_types::packed::CellDep::new_unchecked(dep.as_bytes()).into()
    };

    // TODO: automatic generation
    let l1_sudt_type_dep = gw_types::packed::CellDep::default().into();

    // Allowed eoa script deps
    let mut allowed_eoa_deps = HashMap::new();
    let eth_account_lock_dep = {
        let dep: ckb_types::packed::CellDep =
            scripts_results.eth_account_lock.cell_dep.clone().into();
        gw_types::packed::CellDep::new_unchecked(dep.as_bytes()).into()
    };
    allowed_eoa_deps.insert(
        scripts_results.eth_account_lock.script_type_hash,
        eth_account_lock_dep,
    );

    // Allowed contract script deps
    let mut allowed_contract_deps = HashMap::new();
    let meta_contract_validator_dep = {
        let dep: ckb_types::packed::CellDep = scripts_results
            .meta_contract_validator
            .cell_dep
            .clone()
            .into();
        gw_types::packed::CellDep::new_unchecked(dep.as_bytes()).into()
    };
    allowed_contract_deps.insert(
        scripts_results
            .meta_contract_validator
            .script_type_hash
            .clone(),
        meta_contract_validator_dep,
    );
    let l2_sudt_validator_dep = {
        let dep: ckb_types::packed::CellDep =
            scripts_results.l2_sudt_validator.cell_dep.clone().into();
        gw_types::packed::CellDep::new_unchecked(dep.as_bytes()).into()
    };
    allowed_contract_deps.insert(
        scripts_results.l2_sudt_validator.script_type_hash.clone(),
        l2_sudt_validator_dep,
    );
    let polyjuice_validator_dep = {
        let dep: ckb_types::packed::CellDep =
            scripts_results.polyjuice_validator.cell_dep.clone().into();
        gw_types::packed::CellDep::new_unchecked(dep.as_bytes()).into()
    };
    allowed_contract_deps.insert(
        scripts_results.polyjuice_validator.script_type_hash.clone(),
        polyjuice_validator_dep,
    );

    let challenger_config = ChallengerConfig {
        rewards_receiver_lock: gw_types::packed::Script::default().into(),
        burn_lock: gw_types::packed::Script::default().into(),
    };

    let wallet_config: WalletConfig = WalletConfig {
        privkey_path: privkey_path.into(),
        lock,
    };

    let backends: Vec<BackendConfig> = vec![
        BackendConfig {
            validator_path: scripts_built.get_path("meta_contract_validator"),
            generator_path: scripts_built.get_path("meta_contract_generator"),
            validator_script_type_hash: scripts_results
                .meta_contract_validator
                .script_type_hash
                .clone(),
        },
        BackendConfig {
            validator_path: scripts_built.get_path("l2_sudt_validator"),
            generator_path: scripts_built.get_path("l2_sudt_generator"),
            validator_script_type_hash: scripts_results.l2_sudt_validator.script_type_hash.clone(),
        },
        BackendConfig {
            validator_path: scripts_built.get_path("polyjuice_validator"),
            generator_path: scripts_built.get_path("polyjuice_generator"),
            validator_script_type_hash: scripts_results
                .polyjuice_validator
                .script_type_hash
                .clone(),
        },
    ];
    // FIXME change to a directory path after we tested the persist storage
    let store: StoreConfig = StoreConfig { path: "".into() };
    let genesis_committed_info = L2BlockCommittedInfo {
        block_hash,
        number,
        transaction_hash: genesis.tx_hash,
    };
    let chain: ChainConfig = ChainConfig {
        genesis_committed_info,
        rollup_type_script,
    };
    let rpc_client: RPCClientConfig = RPCClientConfig {
        indexer_url,
        ckb_url,
    };
    let rpc_server = RPCServerConfig { listen: server_url };
    let block_producer: Option<BlockProducerConfig> = Some(BlockProducerConfig {
        account_id,
        // cell deps
        poa_lock_dep,
        poa_state_dep,
        rollup_cell_type_dep,
        rollup_config_cell_dep,
        deposit_cell_lock_dep,
        stake_cell_lock_dep,
        custodian_cell_lock_dep,
        withdrawal_cell_lock_dep,
        challenge_cell_lock_dep,
        l1_sudt_type_dep,
        allowed_eoa_deps,
        allowed_contract_deps,
        challenger_config,
        wallet_config,
    });
    let genesis: GenesisConfig = GenesisConfig {
        timestamp: genesis.timestamp,
        rollup_type_hash,
        meta_contract_validator_type_hash,
        rollup_config,
        secp_data_dep,
    };
    let eth_account_lock_hash = genesis
        .rollup_config
        .allowed_eoa_type_hashes
        .get(0)
        .ok_or_else(|| anyhow!("No allowed EoA type hashes in the rollup config"))?;
    let tron_allowed_eoa_hash = genesis.rollup_config.allowed_eoa_type_hashes.get(1);
    let tron_account_lock_hash = tron_allowed_eoa_hash.map(ToOwned::to_owned);
    let web3_indexer = match database_url {
        Some(database_url) => Some(Web3IndexerConfig {
            database_url: database_url.to_owned(),
            polyjuice_script_type_hash: scripts_results.polyjuice_validator.script_type_hash,
            eth_account_lock_hash: eth_account_lock_hash.to_owned(),
            tron_account_lock_hash,
        }),
        None => None,
    };

    let config: Config = Config {
        backends,
        store,
        genesis,
        chain,
        rpc_client,
        rpc_server,
        block_producer,
        web3_indexer,
        node_mode: NodeMode::ReadOnly,
        debug: Default::default(),
        offchain_validator: Default::default(),
        mem_pool: Default::default(),
        history_validator: Default::default(),
    };

    let output_content = toml::to_string_pretty(&config).expect("serde toml to string pretty");
    fs::write(output_path, output_content.as_bytes()).map_err(|err| anyhow!("{}", err))?;
    Ok(())
}
