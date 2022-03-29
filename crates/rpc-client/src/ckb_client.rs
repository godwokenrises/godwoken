use std::time::Duration;

use crate::{
    error::RPCRequestError,
    utils::{to_result, DEFAULT_HTTP_TIMEOUT},
};
use anyhow::{anyhow, bail, Result};
use async_jsonrpc_client::{HttpClient, Params as ClientParams, Transport};
use gw_jsonrpc_types::blockchain::CellDep;
use serde::de::DeserializeOwned;
use serde_json::json;
use tokio_metrics::TaskMonitor;
use tracing::instrument;

#[derive(Clone)]
pub struct CKBClient {
    ckb_client: HttpClient,
    metrics_monitor: TaskMonitor,
}

impl CKBClient {
    pub fn new(ckb_client: HttpClient) -> Self {
        let metrics_monitor = tokio_metrics::TaskMonitor::new();

        let _metrics_monitor = metrics_monitor.clone();
        tokio::spawn(async move {
            let intervals = _metrics_monitor.intervals();
            for interval in intervals {
                log::debug!("ckb client metrics: {:?}", interval);
                tokio::time::sleep(Duration::from_secs(30)).await;
            }
        });
        Self {
            ckb_client,
            metrics_monitor,
        }
    }

    pub fn with_url(url: &str) -> Result<Self> {
        let client = HttpClient::builder()
            .timeout(DEFAULT_HTTP_TIMEOUT)
            .build(url)?;
        Ok(Self::new(client))
    }

    fn client(&self) -> &HttpClient {
        &self.ckb_client
    }

    #[instrument(skip_all, fields(method = method))]
    pub async fn request<T: DeserializeOwned>(
        &self,
        method: &str,
        params: Option<ClientParams>,
    ) -> Result<T> {
        let monitor = self.metrics_monitor.clone();
        let response = monitor
            .instrument(self.client().request(method, params))
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

    #[instrument(skip_all)]
    pub async fn query_type_script(
        &self,
        contract: &str,
        cell_dep: CellDep,
    ) -> Result<gw_jsonrpc_types::blockchain::Script> {
        use gw_jsonrpc_types::ckb_jsonrpc_types::TransactionWithStatus;

        let tx_hash = cell_dep.out_point.tx_hash;
        let tx_with_status: Option<TransactionWithStatus> = self
            .request(
                "get_transaction",
                Some(ClientParams::Array(vec![json!(tx_hash)])),
            )
            .await?;
        let tx = match tx_with_status {
            Some(tx_with_status) => tx_with_status.transaction.inner,
            None => bail!("{} {} tx not found", contract, tx_hash),
        };

        match tx.outputs.get(cell_dep.out_point.index.value() as usize) {
            Some(output) => match output.type_.as_ref() {
                Some(script) => Ok(script.to_owned().into()),
                None => Err(anyhow!("{} {} tx hasn't type script", contract, tx_hash)),
            },
            None => Err(anyhow!("{} {} tx index not found", contract, tx_hash)),
        }
    }
}
