use std::{fs, path::Path};

use crate::deploy_genesis::GenesisDeploymentResult;
use crate::deploy_scripts::ScriptsDeploymentResult;
use anyhow::{anyhow, Result};
use ckb_sdk::HttpRpcClient;
use ckb_types::prelude::Entity;
use gw_config::{
    BackendConfig, BlockProducerConfig, ChainConfig, Config, GenesisConfig, RPCClientConfig,
    StoreConfig, WalletConfig,
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
    let rollup_cell_lock_dep = {
        let dep: ckb_types::packed::CellDep = scripts.state_validator_lock.cell_dep.clone().into();
        gw_types::packed::CellDep::new_unchecked(dep.as_bytes()).into()
    };
    let rollup_cell_type_dep = {
        let dep: ckb_types::packed::CellDep = scripts.state_validator.cell_dep.clone().into();
        gw_types::packed::CellDep::new_unchecked(dep.as_bytes()).into()
    };
    let deposit_cell_lock_dep = {
        let dep: ckb_types::packed::CellDep = scripts.deposition_lock.cell_dep.clone().into();
        gw_types::packed::CellDep::new_unchecked(dep.as_bytes()).into()
    };

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
        validator_script_type_hash: scripts.polyjuice_validator.script_type_hash,
    });
    let store: StoreConfig = StoreConfig {
        path: "./store.db".into(),
    };
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
    let block_producer: Option<BlockProducerConfig> = Some(BlockProducerConfig {
        account_id,
        // cell deps
        rollup_cell_lock_dep,
        rollup_cell_type_dep,
        deposit_cell_lock_dep,
        wallet_config,
    });
    let genesis: GenesisConfig = GenesisConfig {
        timestamp: genesis.timestamp,
        rollup_type_hash,
        meta_contract_validator_type_hash,
        rollup_config,
    };
    let config: Config = Config {
        backends,
        store,
        genesis,
        chain,
        rpc_client,
        block_producer,
    };
    let output_content = toml::to_string_pretty(&config).expect("serde toml to string pretty");
    fs::write(output_path, output_content.as_bytes()).map_err(|err| anyhow!("{}", err))?;
    Ok(())
}
