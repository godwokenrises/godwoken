use anyhow::Result;
use async_jsonrpc_client::{BatchTransport, HttpClient, Output, Params as ClientParams, Transport};
use ckb_jsonrpc_types::Script;
use ckb_types::H256;
use gw_jsonrpc_types::ckb_jsonrpc_types::Uint32;
use itertools::Itertools;
use serde::de::DeserializeOwned;
use serde_json::{from_value, json};

type AccountID = Uint32;

pub struct GodwokenAsyncClient {
    client: HttpClient,
}

impl GodwokenAsyncClient {
    pub fn new(client: HttpClient) -> Self {
        Self { client }
    }

    pub fn with_url(url: &str) -> Result<Self> {
        let client = HttpClient::builder().build(url)?;
        Ok(Self::new(client))
    }
}

impl GodwokenAsyncClient {
    pub async fn get_script_hash(&self, account_id: u32) -> Result<H256> {
        let script_hash: H256 = self
            .request(
                "gw_get_script_hash",
                Some(ClientParams::Array(vec![json!(AccountID::from(
                    account_id
                ))])),
            )
            .await?;

        Ok(script_hash)
    }

    pub async fn get_script(&self, script_hash: H256) -> Result<Option<Script>> {
        let script: Option<Script> = self
            .request(
                "gw_get_script",
                Some(ClientParams::Array(vec![json!(script_hash)])),
            )
            .await?;

        Ok(script)
    }

    fn client(&self) -> &HttpClient {
        &self.client
    }

    pub async fn get_script_hash_batch(&self, account_ids: Vec<u32>) -> Result<Vec<H256>> {
        let ids = account_ids
            .into_iter()
            .map(|id| {
                (
                    "gw_get_script_hash",
                    Some(ClientParams::Array(vec![json!(AccountID::from(id))])),
                )
            })
            .collect::<Vec<_>>();

        let result: Vec<H256> = self.request_batch(ids).await?;
        Ok(result)
    }

    pub async fn get_script_batch(&self, script_hashes: Vec<H256>) -> Result<Vec<Option<Script>>> {
        let hashes = script_hashes
            .into_iter()
            .map(|h| ("gw_get_script", Some(ClientParams::Array(vec![json!(h)]))))
            .collect::<Vec<_>>();

        let result: Vec<Option<Script>> = self.request_batch(hashes).await?;
        Ok(result)
    }

    async fn request<T: DeserializeOwned>(
        &self,
        method: &str,
        params: Option<ClientParams>,
    ) -> Result<T> {
        let response = self.client().request(method, params).await?;
        let response_str = response.to_string();
        match to_result::<T>(response) {
            Ok(r) => Ok(r),
            Err(err) => {
                log::error!(
                    "[gw_async_client] Failed to parse response, method: {}, response: {}",
                    method,
                    response_str
                );
                Err(err)
            }
        }
    }

    async fn request_batch<T: DeserializeOwned>(
        &self,
        params: Vec<(&str, Option<ClientParams>)>,
    ) -> Result<Vec<T>> {
        let methods = params.iter().map(|p| p.0).unique().collect::<Vec<_>>();

        let responses = self.client().request_batch(params).await?;
        let responses_str = responses.iter().map(|r| r.to_string()).collect::<Vec<_>>();

        let results = responses
            .into_iter()
            .map(|response| match to_result::<T>(response) {
                Ok(r) => Ok(r),
                Err(err) => {
                    log::error!(
                        "[gw_async_client] Failed to parse batch response, methods: {:?}, responses: {:?}",
                        methods,
                        responses_str
                    );
                    Err(err)
                }
            })
            .collect::<Result<Vec<_>>>()?;

        Ok(results)
    }
}

fn to_result<T: DeserializeOwned>(output: Output) -> anyhow::Result<T> {
    match output {
        Output::Success(success) => Ok(from_value(success.result)?),
        Output::Failure(failure) => Err(anyhow::anyhow!("JSONRPC error: {}", failure.error)),
    }
}
