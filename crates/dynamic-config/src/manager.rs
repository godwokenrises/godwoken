use std::sync::Arc;

use anyhow::{anyhow, Result};

use arc_swap::ArcSwap;
use gw_config::{Config, DynamicConfig, FeeConfig};
use gw_tx_filter::{
    erc20_creator_allowlist::SUDTProxyAccountAllowlist,
    polyjuice_contract_creator_allowlist::PolyjuiceContractCreatorAllowList,
};
use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::{fee_config::FeeConfigManager, whitelist_config::WhilteListConfigManager};

// Some configs can be hot reloaded through DynamicConfigManager.
// So that we don't need to restart to take effect every time.
#[derive(Default, Clone)]
pub struct DynamicConfigManager {
    config_github_url: Option<(String, String)>, // url and token

    fee_manager: FeeConfigManager,
    whitelist_manager: WhilteListConfigManager,
}

impl DynamicConfigManager {
    pub fn create(config: Config) -> Self {
        let config_github_url = config.reload_config_github_url.as_ref().map(|r| {
            (
                format!(
                    "https://raw.githubusercontent.com/{}/{}/{}/{}",
                    r.org, r.repo, r.branch, r.path
                ),
                r.token.clone(),
            )
        });
        let fee_manager = FeeConfigManager::create(config.dynamic_config.fee_config.clone());
        let whitelist_manager = WhilteListConfigManager::create(config.dynamic_config.rpc_config);

        Self {
            config_github_url,
            fee_manager,
            whitelist_manager,
        }
    }

    pub async fn reload(&mut self) -> Result<DynamicConfigReloadResponse> {
        // Fetch latest config.
        let new_config = if let Some((url, token)) = &self.config_github_url {
            get_github_config(url, token).await?
        } else {
            return Err(anyhow!("Github config url is absent!"));
        };

        let new_config = new_config.dynamic_config;
        let backup_config = new_config.clone();
        let old_fee_config = self.fee_manager.reload(new_config.fee_config);
        let old_rpc_config = self.whitelist_manager.reload(new_config.rpc_config);
        let old_config = DynamicConfig {
            fee_config: old_fee_config,
            rpc_config: old_rpc_config,
        };
        let res = DynamicConfigReloadResponse {
            old: old_config,
            new: backup_config,
        };
        Ok(res)
    }

    pub fn get_fee_config(&self) -> &FeeConfig {
        self.fee_manager.get_fee_config()
    }

    pub fn get_polyjuice_contract_creator_allowlist(
        &self,
    ) -> &Option<PolyjuiceContractCreatorAllowList> {
        self.whitelist_manager
            .get_polyjuice_contract_creator_allowlist()
    }

    pub fn get_sudt_proxy_account_whitelist(&self) -> &SUDTProxyAccountAllowlist {
        self.whitelist_manager.get_sudt_proxy_account_whitelist()
    }
}

async fn get_github_config(url: &str, token: &str) -> Result<Config> {
    let token = format!("token {}", token);
    let res = Client::builder()
        .build()?
        .get(url)
        .header("Authorization", token)
        .send()
        .await?
        .text()
        .await?;
    let config = toml::from_str(&res)?;
    Ok(config)
}

#[derive(Clone, Default, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DynamicConfigReloadResponse {
    old: DynamicConfig,
    new: DynamicConfig,
}

pub async fn reload(
    manager: Arc<ArcSwap<DynamicConfigManager>>,
) -> Result<DynamicConfigReloadResponse> {
    let mut config = (**manager.load()).to_owned();
    let resp = config.reload().await;
    manager.store(Arc::new(config));
    resp
}

pub async fn try_reload(
    manager: Arc<ArcSwap<DynamicConfigManager>>,
) -> Option<Result<DynamicConfigReloadResponse>> {
    let mut config = (**manager.load()).to_owned();
    match config.config_github_url {
        Some(_) => {
            let resp = config.reload().await;
            manager.store(Arc::new(config));
            Some(resp)
        }
        None => None,
    }
}
