use anyhow::{anyhow, Result};

use gw_config::{Config, FeeConfig};
use gw_tx_filter::{
    erc20_creator_allowlist::SUDTProxyAccountAllowlist,
    polyjuice_contract_creator_allowlist::PolyjuiceContractCreatorAllowList,
};

use crate::{fee_config::FeeConfigManager, whitelist_config::WhilteListConfigManager};

// Some configs can be hot reloaded through DynamicConfigManager.
// So that we don't need to restart to take effect every time.
#[derive(Default)]
pub struct DynamicConfigManager {
    config_github_url: Option<String>,
    fee_manager: FeeConfigManager,
    whitelist_manager: WhilteListConfigManager,
}

impl DynamicConfigManager {
    pub fn create(config: Config) -> Self {
        let config_github_url = config.reload_config_github_url.as_ref().map(|r| {
            format!(
                "https://raw.githubusercontent.com/{}/{}/{}/{}?token={}",
                r.org, r.repo, r.branch, r.path, r.token
            )
        });
        let fee_manager = FeeConfigManager::create(config.fee.clone());
        let whitelist_manager = WhilteListConfigManager::create(config.rpc);

        Self {
            config_github_url,
            fee_manager,
            whitelist_manager,
        }
    }

    pub fn reload(&mut self) -> Result<()> {
        // Fetch latest config.
        let new_config = if let Some(url) = &self.config_github_url {
            get_github_config(url)?
        } else {
            return Err(anyhow!("Github config url is absent!"));
        };

        self.fee_manager.reload(new_config.fee);
        self.whitelist_manager.reload(new_config.rpc);

        Ok(())
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

fn get_github_config(url: &str) -> Result<Config> {
    let res = reqwest::blocking::get(url)?.text()?;
    let config = toml::from_str(&res)?;
    Ok(config)
}
