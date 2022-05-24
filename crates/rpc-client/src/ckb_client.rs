use std::time::{Duration, Instant};

use crate::{
    error::RPCRequestError,
    utils::{to_jsonh256, to_result, DEFAULT_HTTP_TIMEOUT},
};
use anyhow::{anyhow, bail, Result};
use async_jsonrpc_client::{HttpClient, Params as ClientParams, Transport};
use gw_common::H256;
use gw_jsonrpc_types::{
    blockchain::{CellDep, TransactionWithStatus},
    ckb_jsonrpc_types,
};
use gw_types::{offchain::TxStatus, packed::Transaction, prelude::*};
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

    #[instrument(skip_all, fields(tx_hash = %tx_hash.pack()))]
    pub async fn get_transaction_block_hash(&self, tx_hash: H256) -> Result<Option<[u8; 32]>> {
        let tx_with_status = self.get_transaction_with_status(tx_hash).await?;
        Ok(tx_with_status
            .and_then(|tx_with_status| tx_with_status.tx_status.block_hash)
            .map(Into::into))
    }

    #[instrument(skip_all, fields(tx_hash = %tx_hash.pack()))]
    pub async fn get_transaction_block_number(&self, tx_hash: H256) -> Result<Option<u64>> {
        match self.get_transaction_block_hash(tx_hash).await? {
            Some(block_hash) => {
                let block = self.get_block(block_hash.into()).await?;
                Ok(block.map(|b| b.header.inner.number.value()))
            }
            None => Ok(None),
        }
    }

    #[instrument(skip_all, fields(block_hash = %block_hash.pack()))]
    pub async fn get_block(
        &self,
        block_hash: H256,
    ) -> Result<Option<ckb_jsonrpc_types::BlockView>> {
        let block: Option<ckb_jsonrpc_types::BlockView> = self
            .request(
                "get_block",
                Some(ClientParams::Array(vec![json!(to_jsonh256(block_hash))])),
            )
            .await?;

        Ok(block)
    }

    /// Get transaction with status.
    pub async fn get_transaction_with_status(
        &self,
        tx_hash: H256,
    ) -> Result<Option<TransactionWithStatus>> {
        self.request(
            "get_transaction",
            Some(ClientParams::Array(vec![json!(to_jsonh256(tx_hash))])),
        )
        .await
    }

    #[instrument(skip_all, fields(tx_hash = %tx_hash.pack()))]
    pub async fn get_transaction(&self, tx_hash: H256) -> Result<Option<Transaction>> {
        let tx_with_status = self.get_transaction_with_status(tx_hash).await?;
        Ok(tx_with_status
            .and_then(|tx_with_status| tx_with_status.transaction)
            .map(|tv| {
                let tx: ckb_types::packed::Transaction = tv.inner.into();
                Transaction::new_unchecked(tx.as_bytes())
            }))
    }

    #[instrument(skip_all, fields(tx_hash = %tx_hash.pack()))]
    pub async fn get_transaction_status(&self, tx_hash: H256) -> Result<Option<TxStatus>> {
        let tx_with_status = self.get_transaction_with_status(tx_hash).await?;
        Ok(tx_with_status.map(|tx_with_status| tx_with_status.tx_status.status.into()))
    }

    pub async fn wait_tx_proposed(&self, tx_hash: H256) -> Result<()> {
        loop {
            match self.get_transaction_status(tx_hash).await? {
                Some(TxStatus::Proposed) | Some(TxStatus::Committed) => return Ok(()),
                Some(TxStatus::Rejected) => bail!("rejected"),
                _ => (),
            }

            tokio::time::sleep(Duration::new(3, 0)).await;
        }
    }

    pub async fn wait_tx_committed(&self, tx_hash: H256) -> Result<()> {
        loop {
            match self.get_transaction_status(tx_hash).await? {
                Some(TxStatus::Committed) => return Ok(()),
                Some(TxStatus::Rejected) => bail!("rejected"),
                _ => (),
            }

            tokio::time::sleep(Duration::new(3, 0)).await;
        }
    }

    pub async fn wait_tx_committed_with_timeout_and_logging(
        &self,
        tx_hash: H256,
        timeout_secs: u64,
    ) -> Result<()> {
        let timeout = Duration::new(timeout_secs, 0);
        let now = Instant::now();

        loop {
            match self.get_transaction_status(tx_hash).await? {
                Some(TxStatus::Committed) => {
                    log::info!("transaction committed");
                    return Ok(());
                }
                Some(TxStatus::Rejected) => bail!("transaction rejected"),
                Some(status) => log::info!("waiting for transaction, status: {:?}", status),
                None => log::info!("waiting for transaction, not found"),
            }

            if now.elapsed() >= timeout {
                bail!("timeout");
            }

            tokio::time::sleep(Duration::new(3, 0)).await;
        }
    }

    #[instrument(skip_all)]
    pub async fn query_type_script(
        &self,
        contract: &str,
        cell_dep: CellDep,
    ) -> Result<gw_jsonrpc_types::blockchain::Script> {
        use gw_jsonrpc_types::blockchain::TransactionWithStatus;

        let tx_hash = cell_dep.out_point.tx_hash;
        let tx_with_status: Option<TransactionWithStatus> = self
            .request(
                "get_transaction",
                Some(ClientParams::Array(vec![json!(tx_hash)])),
            )
            .await?;
        let tx = match tx_with_status {
            Some(TransactionWithStatus {
                transaction: Some(tv),
                ..
            }) => tv.inner,
            _ => bail!("{} {} tx not found", contract, tx_hash),
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
