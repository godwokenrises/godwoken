use crate::{
    error::RPCRequestError,
    utils::{to_result, DEFAULT_HTTP_TIMEOUT},
};
use anyhow::{Context, Result};
use async_jsonrpc_client::{HttpClient, Params as ClientParams, Transport};
use ckb_fixed_hash::H256;
use gw_jsonrpc_types::blockchain::CellDep;
use gw_jsonrpc_types::ckb_jsonrpc_types::{TransactionView, TxStatus};
use serde::{de::DeserializeOwned, Deserialize};
use serde_json::json;
use tracing::instrument;

#[derive(Clone)]
pub struct CKBClient(HttpClient);

#[derive(Deserialize)]
pub struct TransactionWithStatus {
    pub transaction: Option<TransactionView>,
    pub tx_status: TxStatus,
}

impl CKBClient {
    pub fn new(ckb_client: HttpClient) -> Self {
        Self(ckb_client)
    }

    pub fn with_url(url: &str) -> Result<Self> {
        let client = HttpClient::builder()
            .timeout(DEFAULT_HTTP_TIMEOUT)
            .build(url)?;
        Ok(Self::new(client))
    }

    fn client(&self) -> &HttpClient {
        &self.0
    }

    #[instrument(skip_all, fields(method = method))]
    pub async fn request<T: DeserializeOwned>(
        &self,
        method: &str,
        params: Option<ClientParams>,
    ) -> Result<T> {
        let response = self
            .client()
            .request(method, params)
            .await
            .map_err(|err| RPCRequestError::new("ckb client", method.to_string(), err))?;
        let response_str = response.to_string();
        match to_result::<T>(response) {
            Ok(r) => Ok(r),
            Err(err) => {
                log::error!(
                    "[ckb-client] Failed to parse response, method: {}, response: {}",
                    method,
                    response_str
                );
                Err(err)
            }
        }
    }

    pub async fn get_transaction(&self, tx_hash: &H256) -> Result<TransactionWithStatus> {
        self.request(
            "get_transaction",
            Some(ClientParams::Array(vec![json!(tx_hash)])),
        )
        .await
    }

    #[instrument(skip_all)]
    pub async fn query_type_script(
        &self,
        _contract: &str,
        cell_dep: CellDep,
    ) -> Result<gw_jsonrpc_types::blockchain::Script> {
        let tx_hash = &cell_dep.out_point.tx_hash;
        let tx = self.get_transaction(tx_hash).await?;
        let type_script = tx
            .transaction
            .context("transaction not found")?
            .inner
            .outputs
            .get(cell_dep.out_point.index.value() as usize)
            .cloned()
            .context("output index not found")?
            .type_
            .context("type script not found")?;
        Ok(type_script.into())
    }
}
