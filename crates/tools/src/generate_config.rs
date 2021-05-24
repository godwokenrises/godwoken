use std::{fs, path::Path};

use crate::deploy_genesis::{get_secp_data, GenesisDeploymentResult};
use crate::deploy_scripts::ScriptsDeploymentResult;
use anyhow::{anyhow, Result};
use ckb_sdk::HttpRpcClient;
use ckb_types::prelude::Entity;
use gw_config::{
    BackendConfig, BlockProducerConfig, ChainConfig, Config, GenesisConfig, RPCClientConfig,
    RPCServerConfig, StoreConfig, WalletConfig, Web3IndexerConfig,
};
use gw_jsonrpc_types::godwoken::L2BlockCommittedInfo;

const BACKEND_BINARIES_DIR: &str = "godwoken-scripts/c/build";

pub fn generate_config(
    genesis_path: &Path,
    scripts_path: &Path,
    polyjuice_binaries_dir: &Path,
    ckb_url: String,
    indexer_url: String,
    output_path: &Path,
    database_url: Option<&str>,
) -> Result<()> {
    let genesis: GenesisDeploymentResult = {
        let content = fs::read(genesis_path)?;
        serde_json::from_slice(&content)?
    };
    let scripts: ScriptsDeploymentResult = {
        let content = fs::read(scripts_path)?;
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
    let privkey_path = "<private key path>".into();
    let lock = Default::default();

    let rollup_config = genesis.rollup_config.clone();
    let rollup_type_hash = genesis.rollup_type_hash;
    let meta_contract_validator_type_hash =
        scripts.meta_contract_validator.script_type_hash.clone();
    let rollup_type_script = {
        let script: ckb_types::packed::Script = genesis.rollup_type_script.into();
        gw_types::packed::Script::new_unchecked(script.as_bytes()).into()
    };
    let rollup_config_cell_dep = {
        let cell_dep: ckb_types::packed::CellDep = genesis.rollup_config_cell_dep.into();
        gw_types::packed::CellDep::new_unchecked(cell_dep.as_bytes()).into()
    };
    let poa_lock_dep = {
        let dep: ckb_types::packed::CellDep = scripts.state_validator_lock.cell_dep.clone().into();
        gw_types::packed::CellDep::new_unchecked(dep.as_bytes()).into()
    };
    let poa_state_dep = {
        let dep: ckb_types::packed::CellDep = scripts.poa_state.cell_dep.clone().into();
        gw_types::packed::CellDep::new_unchecked(dep.as_bytes()).into()
    };
    let rollup_cell_type_dep = {
        let dep: ckb_types::packed::CellDep = scripts.state_validator.cell_dep.clone().into();
        gw_types::packed::CellDep::new_unchecked(dep.as_bytes()).into()
    };
    let deposit_cell_lock_dep = {
        let dep: ckb_types::packed::CellDep = scripts.deposit_lock.cell_dep.clone().into();
        gw_types::packed::CellDep::new_unchecked(dep.as_bytes()).into()
    };
    let stake_cell_lock_dep = {
        let dep: ckb_types::packed::CellDep = scripts.stake_lock.cell_dep.clone().into();
        gw_types::packed::CellDep::new_unchecked(dep.as_bytes()).into()
    };
    let (_data, secp_data_dep) =
        get_secp_data(&mut rpc_client).map_err(|err| anyhow!("{}", err))?;
    let custodian_cell_lock_dep = {
        let dep: ckb_types::packed::CellDep = scripts.custodian_lock.cell_dep.clone().into();
        gw_types::packed::CellDep::new_unchecked(dep.as_bytes()).into()
    };
    let withdrawal_cell_lock_dep = {
        let dep: ckb_types::packed::CellDep = scripts.withdrawal_lock.cell_dep.clone().into();
        gw_types::packed::CellDep::new_unchecked(dep.as_bytes()).into()
    };
    // TODO: automatic generation
    let l1_sudt_type_dep = gw_types::packed::CellDep::default().into();

    let wallet_config: WalletConfig = WalletConfig { privkey_path, lock };

    let mut backends: Vec<BackendConfig> = Vec::new();
    backends.push(BackendConfig {
        validator_path: format!("{}/meta-contract-validator", BACKEND_BINARIES_DIR).into(),
        generator_path: format!("{}/meta-contract-generator", BACKEND_BINARIES_DIR).into(),
        validator_script_type_hash: scripts.meta_contract_validator.script_type_hash.clone(),
    });
    backends.push(BackendConfig {
        validator_path: format!("{}/sudt-validator", BACKEND_BINARIES_DIR).into(),
        generator_path: format!("{}/sudt-generator", BACKEND_BINARIES_DIR).into(),
        validator_script_type_hash: scripts.l2_sudt_validator.script_type_hash.clone(),
    });
    let polyjuice_binaries_dir = polyjuice_binaries_dir.to_string_lossy().to_string();
    backends.push(BackendConfig {
        validator_path: format!("{}/polyjuice-validator", polyjuice_binaries_dir).into(),
        generator_path: format!("{}/polyjuice-generator", polyjuice_binaries_dir).into(),
        validator_script_type_hash: scripts.polyjuice_validator.script_type_hash.clone(),
    });
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
    let rpc_server = RPCServerConfig {
        listen: "localhost:8119".to_string(),
    };
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
        l1_sudt_type_dep,
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
    let web3_indexer = match database_url {
        Some(database_url) => Some(Web3IndexerConfig {
            database_url: database_url.to_owned(),
            polyjuice_script_type_hash: scripts.polyjuice_validator.script_type_hash,
            eth_account_lock_hash: eth_account_lock_hash.to_owned(),
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
    };

    let output_content = toml::to_string_pretty(&config).expect("serde toml to string pretty");
    fs::write(output_path, output_content.as_bytes()).map_err(|err| anyhow!("{}", err))?;
    Ok(())
}
