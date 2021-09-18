use gw_common::H256;
use gw_types::offchain::RunResult;
use log::debug;

pub struct SUDTProxyAccountWhitelist {
    allowed_sudt_proxy_creator_account_id: Vec<u32>,
    sudt_proxy_code_hashes: Vec<H256>,
}

impl SUDTProxyAccountWhitelist {
    pub fn new(
        allowed_sudt_proxy_creator_account_id: Vec<u32>,
        sudt_proxy_code_hashes: Vec<H256>,
    ) -> Self {
        Self {
            allowed_sudt_proxy_creator_account_id,
            sudt_proxy_code_hashes,
        }
    }

    /// Only accounts in white list could create sUDT proxy contract.
    pub fn validate(&self, run_result: &RunResult, from_id: u32) -> bool {
        if self.allowed_sudt_proxy_creator_account_id.is_empty()
            || self.sudt_proxy_code_hashes.is_empty()
        {
            return true;
        }
        if self
            .allowed_sudt_proxy_creator_account_id
            .contains(&from_id)
        {
            return true;
        }

        if run_result.new_scripts.is_empty() {
            return true;
        }

        for k in run_result.write_data.keys() {
            debug!(
                "whiltelist: from_id: {:?}, code_hash: {:?}",
                &from_id,
                hex::encode(k.as_slice())
            );

            // Contract create syscall stores code in write_data.
            // check code hash is sudt proxy contract
            if self.sudt_proxy_code_hashes.contains(k) {
                return false;
            }
        }
        true
    }
}

impl Default for SUDTProxyAccountWhitelist {
    fn default() -> Self {
        Self {
            allowed_sudt_proxy_creator_account_id: vec![],
            sudt_proxy_code_hashes: vec![],
        }
    }
}
