use crate::utils::{JsonH256, TracingHttpClient};
use anyhow::Result;
use gw_jsonrpc_types::{ckb_jsonrpc_types::*, godwoken::RegistryAddress};
use jsonrpc_utils::rpc_client;

#[derive(Clone)]
pub struct GWClient {
    pub(crate) inner: TracingHttpClient,
}

#[rpc_client]
impl GWClient {
    pub async fn gw_get_script_hash(&self, id: Uint32) -> Result<JsonH256>;
    pub async fn gw_get_registry_address_by_script_hash(
        &self,
        script_hash: JsonH256,
        registry_id: Uint32,
    ) -> Result<Option<RegistryAddress>>;
}

impl GWClient {
    pub fn with_url(url: &str) -> Result<Self> {
        Ok(Self {
            inner: TracingHttpClient::with_url(url.into())?,
        })
    }

    pub fn url(&self) -> &str {
        self.inner.url()
    }
}
