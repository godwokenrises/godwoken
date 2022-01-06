use gw_config::RPCConfig;
use gw_tx_filter::{
    erc20_creator_allowlist::SUDTProxyAccountAllowlist,
    polyjuice_contract_creator_allowlist::PolyjuiceContractCreatorAllowList,
};

#[derive(Default)]
pub struct WhilteListConfigManager {
    sudt_proxy_account_whitelist: SUDTProxyAccountAllowlist,
    polyjuice_contract_creator_allowlist: Option<PolyjuiceContractCreatorAllowList>,
    rpc_config: RPCConfig,
}

impl WhilteListConfigManager {
    pub fn create(rpc_config: RPCConfig) -> WhilteListConfigManager {
        let (polyjuice_contract_creator_allowlist, sudt_proxy_account_whitelist) =
            get_allow_list(rpc_config.clone());
        Self {
            rpc_config,
            sudt_proxy_account_whitelist,
            polyjuice_contract_creator_allowlist,
        }
    }

    pub(crate) fn get_polyjuice_contract_creator_allowlist(
        &self,
    ) -> &Option<PolyjuiceContractCreatorAllowList> {
        &self.polyjuice_contract_creator_allowlist
    }

    pub(crate) fn get_sudt_proxy_account_whitelist(&self) -> &SUDTProxyAccountAllowlist {
        &self.sudt_proxy_account_whitelist
    }

    pub fn reload(&mut self, rpc_config: RPCConfig) {
        let (polyjuice_contract_creator_allowlist, sudt_proxy_account_whitelist) =
            get_allow_list(rpc_config);
        self.polyjuice_contract_creator_allowlist = polyjuice_contract_creator_allowlist;
        self.sudt_proxy_account_whitelist = sudt_proxy_account_whitelist;
    }
}

fn get_allow_list(
    rpc_config: RPCConfig,
) -> (
    Option<PolyjuiceContractCreatorAllowList>,
    SUDTProxyAccountAllowlist,
) {
    let polyjuice_contract_creator_allowlist =
        PolyjuiceContractCreatorAllowList::from_rpc_config(&rpc_config);

    let sudt_proxy_account_whitelist = SUDTProxyAccountAllowlist::new(
        rpc_config.allowed_sudt_proxy_creator_account_id,
        rpc_config
            .sudt_proxy_code_hashes
            .into_iter()
            .map(|hash| hash.0.into())
            .collect(),
    );
    (
        polyjuice_contract_creator_allowlist,
        sudt_proxy_account_whitelist,
    )
}
