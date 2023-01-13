use std::time::{Duration, Instant};

use crate::utils::{JsonH256, TracingHttpClient};
use anyhow::{anyhow, bail, Result};
use gw_jsonrpc_types::ckb_jsonrpc_types::*;
use gw_types::{h256::H256, packed, prelude::*};
use jsonrpc_utils::rpc_client;
use tracing::instrument;

#[derive(Clone)]
pub struct CkbClient {
    pub(crate) inner: TracingHttpClient,
}

#[rpc_client]
impl CkbClient {
    pub async fn get_block(&self, hash: JsonH256) -> Result<Option<BlockView>>;
    pub async fn get_block_by_number(&self, number: BlockNumber) -> Result<Option<BlockView>>;
    pub async fn get_block_hash(&self, number: BlockNumber) -> Result<Option<JsonH256>>;
    pub async fn get_current_epoch(&self) -> Result<EpochView>;
    pub async fn get_epoch_by_number(&self, number: EpochNumber) -> Result<Option<EpochView>>;
    pub async fn get_header(&self, hash: JsonH256) -> Result<Option<HeaderView>>;
    pub async fn get_header_by_number(&self, number: BlockNumber) -> Result<Option<HeaderView>>;
    pub async fn get_live_cell(
        &self,
        out_point: OutPoint,
        with_data: bool,
    ) -> Result<CellWithStatus>;
    pub async fn get_tip_block_number(&self) -> Result<BlockNumber>;
    pub async fn get_tip_header(&self) -> Result<HeaderView>;
    pub async fn get_transaction(
        &self,
        hash: JsonH256,
        verbosity: Uint32,
    ) -> Result<Option<TransactionWithStatusResponse>>;
    pub async fn get_transaction_proof(
        &self,
        tx_hashes: Vec<JsonH256>,
        block_hash: Option<JsonH256>,
    ) -> Result<TransactionProof>;
    pub async fn verify_transaction_proof(
        &self,
        tx_proof: TransactionProof,
    ) -> Result<Vec<JsonH256>>;
    pub async fn get_fork_block(&self, block_hash: JsonH256) -> Result<Option<BlockView>>;
    pub async fn get_consensus(&self) -> Result<Consensus>;
    pub async fn get_block_median_time(&self, block_hash: JsonH256) -> Result<Option<Timestamp>>;
    pub async fn get_block_economic_state(
        &self,
        block_hash: JsonH256,
    ) -> Result<Option<BlockEconomicState>>;
    pub async fn send_transaction(
        &self,
        tx: Transaction,
        outputs_validator: Option<OutputsValidator>,
    ) -> Result<JsonH256>;
    pub async fn estimate_cycles(&self, tx: Transaction) -> Result<EstimateCycles>;
    pub async fn local_node_info(&self) -> Result<LocalNode>;
    pub async fn get_blockchain_info(&self) -> Result<ChainInfo>;
}

impl CkbClient {
    pub fn with_url(url: &str) -> Result<Self> {
        Ok(Self {
            inner: TracingHttpClient::with_url(url.into())?,
        })
    }

    pub fn url(&self) -> &str {
        self.inner.url()
    }

    pub async fn get_transaction_block_hash(&self, tx_hash: H256) -> Result<Option<[u8; 32]>> {
        let tx_with_status = self.get_transaction(tx_hash.into(), 1.into()).await?;
        Ok(tx_with_status
            .and_then(|tx_with_status| tx_with_status.tx_status.block_hash)
            .map(Into::into))
    }

    pub async fn get_transaction_block_number(&self, tx_hash: H256) -> Result<Option<u64>> {
        match self.get_transaction_block_hash(tx_hash).await? {
            Some(block_hash) => {
                let block = self.get_block(block_hash.into()).await?;
                Ok(block.map(|b| b.header.inner.number.value()))
            }
            None => Ok(None),
        }
    }

    pub async fn get_packed_transaction(
        &self,
        tx_hash: H256,
    ) -> Result<Option<packed::Transaction>> {
        let tx_with_status = self.get_transaction(tx_hash.into(), 2.into()).await?;
        tx_with_status
            .and_then(|tx_with_status| tx_with_status.transaction)
            .map(|tv| {
                let tv = match tv.inner {
                    Either::Left(tv) => tv,
                    Either::Right(_) => bail!("unexpected bytes response for get_transaction"),
                };
                Ok(tv.inner.into())
            })
            .transpose()
    }

    pub async fn get_transaction_status(&self, tx_hash: H256) -> Result<Option<Status>> {
        let tx_with_status = self.get_transaction(tx_hash.into(), 1.into()).await?;
        Ok(tx_with_status.map(|tx_with_status| tx_with_status.tx_status.status))
    }

    pub async fn wait_tx_proposed(&self, tx_hash: H256) -> Result<()> {
        loop {
            match self.get_transaction_status(tx_hash).await? {
                Some(Status::Proposed) | Some(Status::Committed) => return Ok(()),
                Some(Status::Rejected) => bail!("rejected"),
                _ => (),
            }

            tokio::time::sleep(Duration::new(3, 0)).await;
        }
    }

    pub async fn wait_tx_committed(&self, tx_hash: H256) -> Result<()> {
        loop {
            match self.get_transaction_status(tx_hash).await? {
                Some(Status::Committed) => return Ok(()),
                Some(Status::Rejected) => bail!("rejected"),
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
                Some(Status::Committed) => {
                    log::info!("transaction committed");
                    return Ok(());
                }
                Some(Status::Rejected) => bail!("transaction rejected"),
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
    pub async fn query_type_script(&self, contract: &str, cell_dep: CellDep) -> Result<Script> {
        let tx_hash = cell_dep.out_point.tx_hash;
        let tx_with_status: Option<TransactionWithStatusResponse> =
            self.get_transaction(tx_hash.clone(), 2.into()).await?;
        let tx: TransactionView = match tx_with_status {
            Some(TransactionWithStatusResponse {
                transaction: Some(tv),
                ..
            }) => match tv.inner {
                Either::Left(v) => v,
                Either::Right(_v) => unreachable!(),
            },
            _ => bail!("{} {} tx not found", contract, tx_hash),
        };

        match tx
            .inner
            .outputs
            .get(cell_dep.out_point.index.value() as usize)
        {
            Some(output) => match output.type_.as_ref() {
                Some(script) => Ok(script.clone()),
                None => Err(anyhow!("{} {} tx hasn't type script", contract, tx_hash)),
            },
            None => Err(anyhow!("{} {} tx index not found", contract, tx_hash)),
        }
    }
}
