use anyhow::{anyhow, Result};
use ckb_jsonrpc_types::Script;
use ckb_types::H256;
use gw_common::{builtins::ETH_REGISTRY_ACCOUNT_ID, registry_address::RegistryAddress};
use gw_jsonrpc_types::{
    ckb_jsonrpc_types::{JsonBytes, Uint32},
    debugger::{DumpChallengeTarget, ReprMockTransaction},
    godwoken::{RunResult, TxReceipt},
};
use gw_types::U256;
use std::{
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    u32,
};

type AccountID = Uint32;

pub struct GodwokenRpcClient {
    url: reqwest::Url,
    client: reqwest::Client,
    id: Arc<AtomicU64>,
}

impl Clone for GodwokenRpcClient {
    fn clone(&self) -> Self {
        Self {
            url: self.url.clone(),
            client: self.client.clone(),
            id: self.id.clone(),
        }
    }
}

impl GodwokenRpcClient {
    pub fn new(url: &str) -> GodwokenRpcClient {
        let url = reqwest::Url::parse(url).expect("godwoken uri, e.g. \"http://127.0.0.1:8119\"");
        GodwokenRpcClient {
            url,
            id: Arc::new(AtomicU64::new(0)),
            client: reqwest::Client::new(),
        }
    }
}

impl GodwokenRpcClient {
    pub async fn get_tip_block_hash(&self) -> Result<Option<H256>> {
        let params = serde_json::Value::Null;
        self.rpc::<Option<H256>>("get_tip_block_hash", params)
            .await
            .map(|opt| opt.map(Into::into))
    }

    pub async fn get_balance(&self, addr: &RegistryAddress, sudt_id: u32) -> Result<U256> {
        let params = serde_json::to_value((
            JsonBytes::from_vec(addr.to_bytes()),
            AccountID::from(sudt_id),
        ))?;
        self.rpc::<U256>("get_balance", params)
            .await
            .map(Into::into)
    }

    pub async fn get_registry_address_by_script_hash(
        &self,
        script_hash: &H256,
    ) -> Result<Option<RegistryAddress>> {
        let params = serde_json::to_value((script_hash, AccountID::from(ETH_REGISTRY_ACCOUNT_ID)))?;
        let opt_address = self
            .rpc::<Option<gw_jsonrpc_types::godwoken::RegistryAddress>>(
                "get_registry_address_by_script_hash",
                params,
            )
            .await?;
        Ok(opt_address.map(Into::into))
    }

    pub async fn get_script_hash_by_registry_address(
        &mut self,
        addr: &RegistryAddress,
    ) -> Result<H256> {
        let params = serde_json::to_value((JsonBytes::from_vec(addr.to_bytes()),))?;
        self.rpc::<H256>("get_script_hash_by_registry_address", params)
            .await
            .map(Into::into)
    }

    pub async fn get_account_id_by_script_hash(
        &mut self,
        script_hash: H256,
    ) -> Result<Option<u32>> {
        let params = serde_json::to_value((script_hash,))?;
        self.rpc::<Option<Uint32>>("get_account_id_by_script_hash", params)
            .await
            .map(|opt| opt.map(Into::into))
    }

    pub async fn get_nonce(&self, account_id: u32) -> Result<u32> {
        let params = serde_json::to_value((AccountID::from(account_id),))?;
        self.rpc::<Uint32>("get_nonce", params)
            .await
            .map(Into::into)
    }

    pub async fn submit_withdrawal_request(&self, withdrawal_request: JsonBytes) -> Result<H256> {
        let params = serde_json::to_value((withdrawal_request,))?;
        self.rpc::<H256>("submit_withdrawal_request", params)
            .await
            .map(Into::into)
    }

    pub async fn get_script_hash(&self, account_id: u32) -> Result<H256> {
        let params = serde_json::to_value((AccountID::from(account_id),))?;
        self.rpc::<H256>("get_script_hash", params)
            .await
            .map(Into::into)
    }

    pub async fn get_script(&self, script_hash: H256) -> Result<Option<Script>> {
        let params = serde_json::to_value((script_hash,))?;
        self.rpc::<Option<Script>>("get_script", params)
            .await
            .map(|opt| opt.map(Into::into))
    }

    pub async fn submit_l2transaction(&mut self, l2tx: JsonBytes) -> Result<H256> {
        let params = serde_json::to_value((l2tx,))?;
        self.rpc::<H256>("submit_l2transaction", params)
            .await
            .map(Into::into)
    }

    pub async fn execute_l2transaction(&self, l2tx: JsonBytes) -> Result<RunResult> {
        let params = serde_json::to_value((l2tx,))?;
        self.rpc::<RunResult>("execute_l2transaction", params)
            .await
            .map(Into::into)
    }

    pub async fn execute_raw_l2transaction(&self, raw_l2tx: JsonBytes) -> Result<RunResult> {
        let params = serde_json::to_value((raw_l2tx,))?;
        self.rpc::<RunResult>("execute_raw_l2transaction", params)
            .await
            .map(Into::into)
    }

    pub async fn get_transaction_receipt(&self, tx_hash: &H256) -> Result<Option<TxReceipt>> {
        let params = serde_json::to_value((tx_hash,))?;
        self.rpc::<Option<TxReceipt>>("get_transaction_receipt", params)
            .await
            .map(|opt| opt.map(Into::into))
    }

    pub async fn debug_dump_cancel_challenge_tx(
        &self,
        challenge_target: DumpChallengeTarget,
    ) -> Result<ReprMockTransaction> {
        let params = serde_json::to_value((challenge_target,))?;
        self.raw_rpc::<ReprMockTransaction>("debug_dump_cancel_challenge_tx", params)
            .await
            .map(Into::into)
    }

    async fn rpc<SuccessResponse: serde::de::DeserializeOwned>(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<SuccessResponse> {
        let method_name = format!("gw_{}", method);
        self.raw_rpc(&method_name, params)
            .await
            .map_err(|err| anyhow!("{}", err))
    }

    async fn raw_rpc<SuccessResponse: serde::de::DeserializeOwned>(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<SuccessResponse> {
        self.id.fetch_add(1, Ordering::SeqCst);
        let mut req_json = serde_json::Map::new();
        req_json.insert("id".to_owned(), serde_json::to_value(&*self.id).unwrap());
        req_json.insert("jsonrpc".to_owned(), serde_json::to_value(&"2.0").unwrap());
        req_json.insert("method".to_owned(), serde_json::to_value(method).unwrap());
        req_json.insert("params".to_owned(), params);

        let resp = self
            .client
            .post(self.url.clone())
            .json(&req_json)
            .send()
            .await?;
        let output = resp.json::<jsonrpc_core::response::Output>().await?;
        match output {
            jsonrpc_core::response::Output::Success(success) => {
                serde_json::from_value(success.result).map_err(Into::into)
            }
            jsonrpc_core::response::Output::Failure(failure) => Err(failure.error.into()),
        }
    }
}
