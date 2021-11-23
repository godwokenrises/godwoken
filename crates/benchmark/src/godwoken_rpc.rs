use anyhow::Result;
use ckb_jsonrpc_types::{JsonBytes, Script, Uint128, Uint32};
use ckb_types::H256;
use gw_jsonrpc_types::{
    debugger::{DumpChallengeTarget, ReprMockTransaction},
    godwoken::{L2TransactionWithStatus, RunResult, TxReceipt},
};
use std::{io::ErrorKind, u128, u32};

type AccountID = Uint32;

#[derive(Clone)]
pub struct GodwokenRpcClient {
    url: reqwest::Url,
    client: reqwest::Client,
    id: u64,
}

impl GodwokenRpcClient {
    pub fn new(url: reqwest::Url) -> GodwokenRpcClient {
        GodwokenRpcClient {
            url,
            id: 0,
            client: reqwest::Client::new(),
        }
    }
}

impl GodwokenRpcClient {
    pub async fn get_tip_block_hash(&mut self) -> Result<Option<H256>> {
        let params = serde_json::Value::Null;
        self.rpc::<Option<H256>>("get_tip_block_hash", params)
            .await
            .map(|opt| opt.map(Into::into))
    }

    pub async fn get_balance(&mut self, short_address: JsonBytes, sudt_id: u32) -> Result<u128> {
        let params = serde_json::to_value((short_address, AccountID::from(sudt_id)))?;
        self.rpc::<Uint128>("get_balance", params)
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

    pub async fn get_nonce(&mut self, account_id: u32) -> Result<u32> {
        let params = serde_json::to_value((AccountID::from(account_id),))?;
        self.rpc::<Uint32>("get_nonce", params)
            .await
            .map(Into::into)
    }

    pub async fn submit_withdrawal_request(&mut self, withdrawal_request: JsonBytes) -> Result<()> {
        let params = serde_json::to_value((withdrawal_request,))?;
        self.rpc::<()>("submit_withdrawal_request", params)
            .await
            .map(Into::into)
    }

    pub async fn get_script_hash(&mut self, account_id: u32) -> Result<H256> {
        let params = serde_json::to_value((AccountID::from(account_id),))?;
        self.rpc::<H256>("get_script_hash", params)
            .await
            .map(Into::into)
    }

    pub async fn get_script(&mut self, script_hash: H256) -> Result<Option<Script>> {
        let params = serde_json::to_value((script_hash,))?;
        self.rpc::<Option<Script>>("get_script", params)
            .await
            .map(|opt| opt.map(Into::into))
    }

    pub async fn get_script_hash_by_short_address(
        &mut self,
        short_address: JsonBytes,
    ) -> Result<Option<H256>> {
        let params = serde_json::to_value((short_address,))?;

        self.rpc::<Option<H256>>("get_script_hash_by_short_address", params)
            .await
            .map(|opt| opt.map(Into::into))
    }

    pub async fn submit_l2transaction(&mut self, l2tx: JsonBytes) -> Result<H256> {
        let params = serde_json::to_value((l2tx,))?;
        self.rpc::<H256>("submit_l2transaction", params)
            .await
            .map(Into::into)
    }

    pub async fn execute_l2transaction(&mut self, l2tx: JsonBytes) -> Result<RunResult> {
        let params = serde_json::to_value((l2tx,))?;
        self.rpc::<RunResult>("execute_l2transaction", params).await
    }

    pub async fn execute_raw_l2transaction(&mut self, raw_l2tx: JsonBytes) -> Result<RunResult> {
        let params = serde_json::to_value((raw_l2tx,))?;
        self.rpc::<RunResult>("execute_raw_l2transaction", params)
            .await
    }

    pub async fn get_transaction(
        &mut self,
        tx_hash: &H256,
    ) -> Result<Option<L2TransactionWithStatus>> {
        let params = serde_json::to_value((tx_hash,))?;
        self.rpc::<Option<L2TransactionWithStatus>>("get_transaction", params)
            .await
            .map(|opt| opt.map(Into::into))
    }

    pub async fn get_transaction_receipt(&mut self, tx_hash: &H256) -> Result<Option<TxReceipt>> {
        let params = serde_json::to_value((tx_hash,))?;
        self.rpc::<Option<TxReceipt>>("get_transaction_receipt", params)
            .await
            .map(|opt| opt.map(Into::into))
    }

    pub async fn debug_dump_cancel_challenge_tx(
        &mut self,
        challenge_target: DumpChallengeTarget,
    ) -> Result<ReprMockTransaction> {
        let params = serde_json::to_value((challenge_target,))?;
        self.rpc::<ReprMockTransaction>("debug_dump_cancel_challenge_tx", params)
            .await
    }

    async fn rpc<SuccessResponse: serde::de::DeserializeOwned>(
        &mut self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<SuccessResponse> {
        let method_name = format!("gw_{}", method);
        Ok(self
            .raw_rpc(&method_name, params)
            .await
            .map_err(|err| std::io::Error::new(ErrorKind::Other, err))?)
    }

    async fn raw_rpc<SuccessResponse: serde::de::DeserializeOwned>(
        &mut self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<SuccessResponse, String> {
        self.id += 1;
        let mut req_json = serde_json::Map::new();
        req_json.insert("id".to_owned(), serde_json::to_value(&self.id).unwrap());
        req_json.insert("jsonrpc".to_owned(), serde_json::to_value(&"2.0").unwrap());
        req_json.insert("method".to_owned(), serde_json::to_value(method).unwrap());
        req_json.insert("params".to_owned(), params);

        let resp = self
            .client
            .post(self.url.clone())
            .json(&req_json)
            .send()
            .await
            .map_err(|err| err.to_string())?;
        let output = resp
            .json::<jsonrpc_core::response::Output>()
            .await
            .map_err(|err| err.to_string())?;
        match output {
            jsonrpc_core::response::Output::Success(success) => {
                serde_json::from_value(success.result).map_err(|err| err.to_string())
            }
            jsonrpc_core::response::Output::Failure(failure) => Err(failure.error.to_string()),
        }
    }
}
