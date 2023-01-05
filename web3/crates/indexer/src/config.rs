use std::{env, fmt, fmt::Display, path::Path};

use anyhow::Result;
use ckb_types::H256;
use dotenv;
use gw_jsonrpc_types::godwoken::{BackendType, EoaScriptType, GwScriptType};
use gw_web3_rpc_client::godwoken_rpc_client::GodwokenRpcClient;
use serde::{Deserialize, Serialize};

#[derive(Clone, Default, Debug, PartialEq, Serialize, Deserialize)]
pub struct IndexerConfig {
    pub l2_sudt_type_script_hash: H256,
    pub polyjuice_type_script_hash: H256,
    pub rollup_type_hash: H256,
    pub eth_account_lock_hash: H256,
    pub godwoken_rpc_url: String,
    pub pg_url: String,
    pub chain_id: u64,
    pub sentry_dsn: Option<String>,
    pub sentry_environment: Option<String>,
}

impl Display for IndexerConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "IndexerConfig {{ ")?;
        write!(
            f,
            "l2_sudt_type_script_hash: 0x{}, ",
            self.l2_sudt_type_script_hash
        )?;
        write!(
            f,
            "polyjuice_type_script_hash: 0x{}, ",
            self.polyjuice_type_script_hash
        )?;
        write!(f, "rollup_type_hash: 0x{}, ", self.rollup_type_hash)?;
        write!(
            f,
            "eth_account_lock_hash: 0x{}, ",
            self.eth_account_lock_hash
        )?;
        write!(f, "godwoken_rpc_url: {}, ", self.godwoken_rpc_url)?;
        write!(f, "pg_url: {}", self.pg_url)?;
        write!(f, "chain_id: {}", self.chain_id)?;
        if let Some(t) = &self.sentry_dsn {
            write!(f, "sentry_dsn: {}, ", t)?;
        } else {
            write!(f, "sentry_dsn: null, ")?;
        }
        if let Some(t) = &self.sentry_environment {
            write!(f, "sentry_environment: {}, ", t)?;
        } else {
            write!(f, "sentry_environment: null, ")?;
        }
        write!(f, " }}")
    }
}

// TODO: rename the configuration file from "indexer-config.toml" to ".env"
// TODO: uppercase the environment variables name "pg_url", "godwoken_rpc_url", etc
pub fn load_indexer_config<P: AsRef<Path>>(path: P) -> Result<IndexerConfig> {
    if path.as_ref().exists() {
        log::info!(
            "Loading configuration file {}",
            path.as_ref().to_string_lossy().to_string()
        );
        dotenv::from_path(path)?;
    } else {
        log::info!(
            "Cannot find configuration file {}, continue",
            path.as_ref().to_string_lossy().to_string()
        );
    }

    // Load components configurations from environment variables
    let pg_url = env::var("pg_url").expect("env var \"pg_url\" is required");
    let godwoken_rpc_url =
        env::var("godwoken_rpc_url").unwrap_or_else(|_| "http://127.0.0.1:8119".to_string());
    let sentry_dsn = env::var("sentry_dsn").ok();
    let sentry_environment = env::var("sentry_environment").ok();

    // Load chain spec via gw_get_node_info
    let godwoken_rpc_client = GodwokenRpcClient::new(&godwoken_rpc_url);
    let godwoken_node_info = godwoken_rpc_client.get_node_info()?;
    let l2_sudt_type_script_hash = godwoken_node_info
        .gw_scripts
        .iter()
        .find_map(|gw_script| {
            if gw_script.script_type == GwScriptType::L2Sudt {
                Some(gw_script.type_hash.clone())
            } else {
                None
            }
        })
        .unwrap();
    let polyjuice_type_script_hash = godwoken_node_info
        .backends
        .iter()
        .find_map(|backend_info| {
            if backend_info.backend_type == BackendType::Polyjuice {
                Some(backend_info.validator_script_type_hash.clone())
            } else {
                None
            }
        })
        .unwrap();
    let eth_account_lock_hash = godwoken_node_info
        .eoa_scripts
        .iter()
        .find_map(|eoa_script| {
            if eoa_script.eoa_type == EoaScriptType::Eth {
                Some(eoa_script.type_hash.clone())
            } else {
                None
            }
        })
        .unwrap();
    let rollup_type_hash = godwoken_node_info.rollup_cell.type_hash.clone();
    let chain_id = godwoken_node_info.rollup_config.chain_id.value();

    Ok(IndexerConfig {
        l2_sudt_type_script_hash,
        polyjuice_type_script_hash,
        rollup_type_hash,
        eth_account_lock_hash,
        godwoken_rpc_url,
        pg_url,
        chain_id,
        sentry_dsn,
        sentry_environment,
    })
}
