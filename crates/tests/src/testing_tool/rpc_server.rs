use std::{collections::HashSet, iter::FromIterator, sync::Arc, time::Duration};

use anyhow::{bail, Result};
use gw_chain::chain::Chain;
use gw_config::{NodeMode::FullNode, RPCClientConfig, RPCMethods};
use gw_jsonrpc_types::{
    ckb_jsonrpc_types::{JsonBytes, Uint64},
    godwoken::{MolJsonBytes, RunResult},
};
use gw_polyjuice_sender_recover::recover::PolyjuiceSenderRecover;
use gw_rpc_client::{
    ckb_client::CkbClient, indexer_client::CkbIndexerClient, rpc_client::RPCClient,
};
use gw_rpc_server::registry::{GwRpc, Registry, RegistryArgs};
use gw_types::{
    bytes::Bytes,
    h256::*,
    packed::{L2Transaction, RawL2Transaction, Script, WithdrawalRequestExtra},
    prelude::*,
};
use gw_utils::wallet::Wallet;
use jsonrpc_core::Result as RpcResult;

use super::chain::{chain_generator, TestChain};

pub struct RPCServer {
    inner: Arc<Registry>,
}

impl RPCServer {
    pub fn default_registry_args(
        chain: &Chain,
        rollup_type_script: Script,
        creator_wallet: Option<Wallet>,
    ) -> RegistryArgs {
        let store = chain.store().clone();
        let mem_pool = chain.mem_pool().clone();
        let generator = chain_generator(chain, rollup_type_script.clone());
        let rollup_config = generator.rollup_context().rollup_config.to_owned();
        let rollup_context = generator.rollup_context().to_owned();
        let rpc_client = {
            let ckb_client = CkbClient::with_url(&RPCClientConfig::default().ckb_url).unwrap();
            let indexer_client = CkbIndexerClient::from(ckb_client.clone());
            let rollup_type_script =
                ckb_types::packed::Script::new_unchecked(rollup_type_script.as_bytes());
            RPCClient::new(
                rollup_type_script,
                rollup_context.rollup_config,
                ckb_client,
                indexer_client,
            )
        };

        let polyjuice_sender_recover =
            PolyjuiceSenderRecover::create(generator.rollup_context(), creator_wallet).unwrap();

        RegistryArgs {
            store,
            mem_pool,
            generator,
            tests_rpc_impl: None,
            rollup_config,
            mem_pool_config: Default::default(),
            node_mode: FullNode,
            rpc_client,
            send_tx_rate_limit: Default::default(),
            server_config: gw_config::RPCServerConfig {
                enable_methods: HashSet::from_iter(vec![RPCMethods::Test]),
                ..Default::default()
            },
            chain_config: Default::default(),
            consensus_config: Default::default(),
            dynamic_config_manager: Default::default(),
            gasless_tx_support_config: None,
            polyjuice_sender_recover,
            debug_backend_forks: None,
        }
    }

    pub async fn build_from_registry_args(registry_args: RegistryArgs) -> Result<Self> {
        let server = RPCServer {
            inner: Registry::create(registry_args).await?,
        };

        Ok(server)
    }

    pub async fn build(chain: &TestChain, creator_wallet: Option<Wallet>) -> Result<Self> {
        let rollup_type_script = chain.rollup_type_script.to_owned();
        let registry_args =
            Self::default_registry_args(&chain.inner, rollup_type_script, creator_wallet);
        Self::build_from_registry_args(registry_args).await
    }

    pub async fn submit_l2transaction(&self, tx: &L2Transaction) -> RpcResult<Option<H256>> {
        let r = self
            .inner
            .gw_submit_l2transaction(MolJsonBytes(tx.clone()))
            .await?;
        Ok(r.map(Into::into))
    }

    pub async fn execute_l2transaction(&self, tx: &L2Transaction) -> RpcResult<RunResult> {
        let r = self
            .inner
            .gw_execute_l2transaction(MolJsonBytes(tx.clone()))
            .await?;
        Ok(r)
    }

    pub async fn execute_raw_l2transaction(
        &self,
        raw_tx: &RawL2Transaction,
        opt_block_number: Option<u64>,
        opt_registry_address: Option<Bytes>,
    ) -> RpcResult<RunResult> {
        let params = serde_json::to_value(&(
            MolJsonBytes(raw_tx.clone()),
            opt_block_number.map(Uint64::from),
            opt_registry_address.map(JsonBytes::from_bytes),
        ))
        .unwrap();
        let (a, b, c) = serde_json::from_value(params)
            .map_err(|e| jsonrpc_core::Error::invalid_params(e.to_string()))?;
        let r = self.inner.gw_execute_raw_l2transaction(a, b, c).await?;
        Ok(r)
    }

    pub async fn is_request_in_queue(&self, hash: H256) -> RpcResult<bool> {
        let result = self.inner.gw_is_request_in_queue(hash.into()).await?;
        Ok(result)
    }

    pub async fn submit_withdrawal_request(&self, req: &WithdrawalRequestExtra) -> RpcResult<H256> {
        let r = self
            .inner
            .gw_submit_withdrawal_request(MolJsonBytes(req.clone()))
            .await?;

        Ok(r.into())
    }
}

pub async fn wait_tx_committed(chain: &TestChain, tx_hash: &H256, timeout: Duration) -> Result<()> {
    let now = std::time::Instant::now();

    loop {
        tokio::time::sleep(Duration::from_secs(1)).await;

        {
            let mem_pool = chain.mem_pool().await;
            if mem_pool.mem_block().txs_set().contains(tx_hash) {
                return Ok(());
            }
            if now.elapsed() > timeout {
                bail!("wait tx {:x} commit timeout", tx_hash.pack());
            }
        }
    }
}
