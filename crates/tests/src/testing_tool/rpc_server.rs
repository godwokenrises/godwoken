use std::{sync::Arc, time::Duration};

use anyhow::{bail, Result};
use ckb_types::prelude::Entity;
use gw_block_producer::test_mode_control::TestModeControl;
use gw_chain::chain::Chain;
use gw_common::H256;
use gw_config::{NodeMode::FullNode, RPCClientConfig};

use gw_jsonrpc_types::ckb_jsonrpc_types::{Byte32, JsonBytes};
use gw_rpc_client::{
    ckb_client::CKBClient, indexer_client::CKBIndexerClient, rpc_client::RPCClient,
};
use gw_rpc_server::registry::{Registry, RegistryArgs};
use gw_types::{
    packed::{L2Transaction, Script},
    prelude::Pack,
};

use jsonrpc_v2::{
    MapRouter, RequestBuilder, RequestObject, ResponseObject, ResponseObjects, Server,
};
use serde::de::DeserializeOwned;

use crate::testing_tool::chain::chain_generator;

pub struct RPCServer {
    inner: Arc<Server<MapRouter>>,
}

impl RPCServer {
    pub fn default_registry_args(
        chain: &Chain,
        rollup_type_script: Script,
    ) -> RegistryArgs<TestModeControl> {
        let store = chain.store().clone();
        let mem_pool = chain.mem_pool().clone();
        let generator = chain_generator(&chain, rollup_type_script.clone());
        let rollup_config = generator.rollup_context().rollup_config.to_owned();
        let rollup_context = generator.rollup_context().to_owned();
        let rpc_client = {
            let indexer_client =
                CKBIndexerClient::with_url(&RPCClientConfig::default().indexer_url).unwrap();
            let ckb_client = CKBClient::with_url(&RPCClientConfig::default().ckb_url).unwrap();
            let rollup_type_script =
                ckb_types::packed::Script::new_unchecked(rollup_type_script.as_bytes());
            RPCClient::new(
                rollup_type_script,
                rollup_context,
                ckb_client,
                indexer_client,
            )
        };

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
            server_config: Default::default(),
            chain_config: Default::default(),
            consensus_config: Default::default(),
            dynamic_config_manager: Default::default(),
            last_submitted_tx_hash: None,
        }
    }

    pub async fn build_from_registry_args(
        registry_args: RegistryArgs<TestModeControl>,
    ) -> Result<Self> {
        let server = RPCServer {
            inner: Registry::create(registry_args).await.build_rpc_server()?,
        };

        Ok(server)
    }

    pub async fn build(chain: &Chain, rollup_type_script: Script) -> Result<Self> {
        let registry_args = Self::default_registry_args(chain, rollup_type_script);
        Self::build_from_registry_args(registry_args).await
    }

    pub async fn submit_l2transaction(&self, tx: &L2Transaction) -> Result<H256> {
        let params = {
            let bytes = JsonBytes::from_bytes(tx.as_bytes());
            serde_json::to_value(&(bytes,))?
        };

        let req = RequestBuilder::default()
            .with_id(1)
            .with_method("gw_submit_l2transaction")
            .with_params(params)
            .finish();

        let tx_hash: Byte32 = self.handle_single_request(req).await?;
        Ok(tx_hash.0.into())
    }

    async fn handle_single_request<R: DeserializeOwned>(&self, req: RequestObject) -> Result<R> {
        let ret = match self.inner.handle(req).await {
            ResponseObjects::One(ResponseObject::Result { result, .. }) => {
                serde_json::to_value(result)?
            }
            ResponseObjects::One(ResponseObject::Error { error, .. }) => {
                bail!(serde_json::to_string(&error)?)
            }
            ResponseObjects::Empty => serde_json::to_value(ResponseObjects::Empty)?,
            ResponseObjects::Many(_) => unreachable!(),
        };

        Ok(serde_json::from_value(ret)?)
    }
}

pub async fn wait_tx_committed(chain: &Chain, tx_hash: &H256, timeout: Duration) -> Result<()> {
    let now = std::time::Instant::now();

    loop {
        tokio::time::sleep(Duration::from_secs(1)).await;

        {
            let mem_pool = chain.mem_pool().as_ref().unwrap().lock().await;
            if mem_pool.mem_block().txs_set().contains(&tx_hash) {
                return Ok(());
            }
            if now.elapsed() > timeout {
                bail!("wait tx {:x} commit timeout", tx_hash.pack());
            }
        }
    }
}
