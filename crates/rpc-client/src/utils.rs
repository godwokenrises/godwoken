use std::time::Duration;

use anyhow::Result;
use jsonrpc_utils::HttpClient;
use reqwest::Client;
use tracing::{field, instrument, Span};

pub(crate) const DEFAULT_QUERY_LIMIT: usize = 500;

pub(crate) type JsonH256 = ckb_fixed_hash::H256;

const DEFAULT_HTTP_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Clone)]
pub struct TracingHttpClient {
    pub(crate) inner: HttpClient,
}

impl TracingHttpClient {
    pub fn with_url(url: String) -> Result<Self> {
        Ok(Self {
            inner: HttpClient::with_client(
                url,
                Client::builder().timeout(DEFAULT_HTTP_TIMEOUT).build()?,
            ),
        })
    }

    pub fn url(&self) -> &str {
        self.inner.url()
    }

    #[instrument(target = "gw-rpc-client", skip_all, err, fields(method, params = field::Empty))]
    pub async fn rpc(
        &self,
        method: &str,
        params: &serde_json::value::RawValue,
    ) -> Result<serde_json::Value> {
        if params.get().len() < 64 {
            Span::current().record("params", field::display(&params));
        }

        self.inner.rpc(method, params).await
    }
}
