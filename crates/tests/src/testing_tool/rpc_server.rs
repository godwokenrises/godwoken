use std::{sync::Arc, time::Duration};

use anyhow::{bail, Result};
use ckb_types::prelude::Entity;
use gw_block_producer::test_mode_control::TestModeControl;
use gw_chain::chain::Chain;
use gw_common::H256;
use gw_config::{NodeMode::FullNode, RPCClientConfig};

use gw_jsonrpc_types::{
    ckb_jsonrpc_types::{Byte32, JsonBytes, Uint64},
    godwoken::RunResult,
};
use gw_polyjuice_sender_recover::recover::PolyjuiceSenderRecover;
use gw_rpc_client::{
    ckb_client::CKBClient, indexer_client::CKBIndexerClient, rpc_client::RPCClient,
};
use gw_rpc_server::registry::{Registry, RegistryArgs};
use gw_types::{
    bytes::Bytes,
    packed::{L2Transaction, RawL2Transaction, Script},
    prelude::Pack,
};

use gw_utils::wallet::Wallet;
use jsonrpc_v2::{
    MapRouter, RequestBuilder, RequestObject, ResponseObject, ResponseObjects, Server,
};
use serde::de::DeserializeOwned;

use crate::testing_tool::chain::chain_generator;

use super::chain::TestChain;

pub struct RPCServer {
    inner: Arc<Server<MapRouter>>,
}

impl RPCServer {
    pub fn default_registry_args(
        chain: &Chain,
        rollup_type_script: Script,
        creator_wallet: Option<Wallet>,
    ) -> RegistryArgs<TestModeControl> {
        let store = chain.store().clone();
        let mem_pool = chain.mem_pool().clone();
        let generator = chain_generator(chain, rollup_type_script.clone());
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
            server_config: Default::default(),
            chain_config: Default::default(),
            consensus_config: Default::default(),
            dynamic_config_manager: Default::default(),
            last_submitted_tx_hash: None,
            polyjuice_sender_recover,
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

    pub async fn build(chain: &TestChain, creator_wallet: Option<Wallet>) -> Result<Self> {
        let rollup_type_script = chain.rollup_type_script.to_owned();
        let registry_args =
            Self::default_registry_args(&chain.inner, rollup_type_script, creator_wallet);
        Self::build_from_registry_args(registry_args).await
    }

    pub async fn submit_l2transaction(&self, tx: &L2Transaction) -> Result<Option<H256>> {
        let params = {
            let bytes = JsonBytes::from_bytes(tx.as_bytes());
            serde_json::to_value(&(bytes,))?
        };

        let req = RequestBuilder::default()
            .with_id(1)
            .with_method("gw_submit_l2transaction")
            .with_params(params)
            .finish();

        let tx_hash: Option<Byte32> = self.handle_single_request(req).await?;
        Ok(tx_hash.map(|h| h.0.into()))
    }

    pub async fn execute_l2transaction(&self, tx: &L2Transaction) -> Result<RunResult> {
        let params = {
            let bytes = JsonBytes::from_bytes(tx.as_bytes());
            serde_json::to_value(&(bytes,))?
        };

        let req = RequestBuilder::default()
            .with_id(1)
            .with_method("gw_execute_l2transaction")
            .with_params(params)
            .finish();

        let run_result = self.handle_single_request(req).await?;
        Ok(run_result)
    }

    pub async fn execute_raw_l2transaction(
        &self,
        raw_tx: &RawL2Transaction,
        opt_block_number: Option<u64>,
        opt_registry_address: Option<Bytes>,
    ) -> Result<RunResult> {
        let raw_tx_bytes = JsonBytes::from_bytes(raw_tx.as_bytes());
        let params = match (opt_block_number, opt_registry_address) {
            (None, None) => serde_json::to_value(&(raw_tx_bytes))?,
            (Some(block_number), None) => {
                let block_number: Uint64 = block_number.into();
                serde_json::to_value(&(raw_tx_bytes, block_number))?
            }
            (Some(block_number), Some(registry_address_bytes)) => {
                let block_number: Uint64 = block_number.into();
                let address_bytes = JsonBytes::from_bytes(registry_address_bytes);
                serde_json::to_value(&(raw_tx_bytes, block_number, address_bytes))?
            }
            (None, Some(registry_address_bytes)) => {
                let address_bytes = JsonBytes::from_bytes(registry_address_bytes);
                serde_json::to_value(&(raw_tx_bytes, Option::<Uint64>::None, address_bytes))?
            }
        };

        let req = RequestBuilder::default()
            .with_id(1)
            .with_method("gw_execute_raw_l2transaction")
            .with_params(params)
            .finish();

        let run_result = self.handle_single_request(req).await?;
        Ok(run_result)
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
