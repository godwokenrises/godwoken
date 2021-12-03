use std::path::PathBuf;

use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Config {
    pub interval: u64,
    pub batch: u16,
    pub timeout: u64,
    pub account_path: PathBuf,
    pub gw_rpc_url: String,
    pub polyman_url: String,
    pub scripts_deploy_path: PathBuf,
    pub rollup_type_hash: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            interval: 1000,
            batch: 10,
            timeout: 120,
            account_path: PathBuf::from("./accounts"),
            gw_rpc_url: String::from("http://localhost:8119"),
            polyman_url: String::from("http://localhost:6102"),
            scripts_deploy_path: PathBuf::from("./scripts_deploy_results.json"),
            rollup_type_hash: String::from("0x"),
        }
    }
}
