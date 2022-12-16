use anyhow::{anyhow, Result};
use ckb_jsonrpc_types::Script;
use ckb_types::H256;
use gw_jsonrpc_types::godwoken::NodeInfo;
use gw_jsonrpc_types::{
    ckb_jsonrpc_types::{JsonBytes, Uint128, Uint32, Uint64},
    debugger::{DumpChallengeTarget, ReprMockTransaction},
    godwoken::{L2BlockView, L2BlockWithStatus, RunResult, TxReceipt},
};
use rand::Rng;
use std::{u128, u32};

use crate::error::RpcClientError;

type AccountID = Uint32;

type RpcClientResult<T> = Result<T, RpcClientError>;

pub struct GodwokenRpcClient {
    url: reqwest::Url,
    client: reqwest::blocking::Client,
}

impl GodwokenRpcClient {
    pub fn new(url: &str) -> GodwokenRpcClient {
        let url = reqwest::Url::parse(url).expect("godwoken uri, e.g. \"http://127.0.0.1:8119\"");
        GodwokenRpcClient {
            url,
            client: reqwest::blocking::Client::new(),
        }
    }
}

impl GodwokenRpcClient {
    pub fn get_tip_block_hash(&self) -> RpcClientResult<Option<H256>> {
        let params = serde_json::Value::Null;
        self.rpc::<Option<H256>>("get_tip_block_hash", params)
            .map(|opt| opt.map(Into::into))
    }

    pub fn get_balance(&self, registry_address: JsonBytes, sudt_id: u32) -> RpcClientResult<u128> {
        let params = serde_json::to_value((registry_address, AccountID::from(sudt_id)))?;
        self.rpc::<Uint128>("get_balance", params).map(Into::into)
    }

    pub fn get_account_id_by_script_hash(&self, script_hash: H256) -> RpcClientResult<Option<u32>> {
        let params = serde_json::to_value((script_hash,))?;
        self.rpc::<Option<Uint32>>("get_account_id_by_script_hash", params)
            .map(|opt| opt.map(Into::into))
    }

    pub fn get_nonce(&self, account_id: u32) -> RpcClientResult<u32> {
        let params = serde_json::to_value((AccountID::from(account_id),))?;
        self.rpc::<Uint32>("get_nonce", params).map(Into::into)
    }

    pub fn submit_withdrawal_request(&self, withdrawal_request: JsonBytes) -> RpcClientResult<()> {
        let params = serde_json::to_value((withdrawal_request,))?;
        self.rpc::<()>("submit_withdrawal_request", params)
            .map(Into::into)
    }

    pub fn get_script_hash(&self, account_id: u32) -> RpcClientResult<H256> {
        let params = serde_json::to_value((AccountID::from(account_id),))?;
        self.rpc::<H256>("get_script_hash", params).map(Into::into)
    }

    pub fn get_script(&self, script_hash: H256) -> RpcClientResult<Option<Script>> {
        let params = serde_json::to_value((script_hash,))?;
        self.rpc::<Option<Script>>("get_script", params)
            .map(|opt| opt.map(Into::into))
    }

    pub fn submit_l2transaction(&self, l2tx: JsonBytes) -> RpcClientResult<H256> {
        let params = serde_json::to_value((l2tx,))?;
        self.rpc::<H256>("submit_l2transaction", params)
            .map(Into::into)
    }

    pub fn execute_l2transaction(&self, l2tx: JsonBytes) -> RpcClientResult<RunResult> {
        let params = serde_json::to_value((l2tx,))?;
        self.rpc::<RunResult>("execute_l2transaction", params)
            .map(Into::into)
    }

    pub fn execute_raw_l2transaction(&self, raw_l2tx: JsonBytes) -> RpcClientResult<RunResult> {
        let params = serde_json::to_value((raw_l2tx,))?;
        self.rpc::<RunResult>("execute_raw_l2transaction", params)
            .map(Into::into)
    }

    pub fn get_transaction_receipt(&self, tx_hash: &H256) -> RpcClientResult<Option<TxReceipt>> {
        let params = serde_json::to_value((tx_hash,))?;
        self.rpc::<Option<TxReceipt>>("get_transaction_receipt", params)
            .map(|opt| opt.map(Into::into))
    }

    pub fn get_block(&self, block_hash: &H256) -> RpcClientResult<Option<L2BlockWithStatus>> {
        let params = serde_json::to_value((block_hash,))?;
        self.rpc::<Option<L2BlockWithStatus>>("get_block", params)
            .map(|opt| opt.map(Into::into))
    }

    pub fn get_block_by_number(&self, block_number: u64) -> RpcClientResult<Option<L2BlockView>> {
        let params = serde_json::to_value((Uint64::from(block_number),))?;
        self.rpc::<Option<L2BlockView>>("get_block_by_number", params)
            .map(|opt| opt.map(Into::into))
    }

    pub fn get_node_info(&self) -> RpcClientResult<NodeInfo> {
        let params = serde_json::Value::Null;
        self.rpc::<NodeInfo>("get_node_info", params)
    }

    pub fn debug_dump_cancel_challenge_tx(
        &self,
        challenge_target: DumpChallengeTarget,
    ) -> Result<ReprMockTransaction> {
        let params = serde_json::to_value((challenge_target,))?;
        self.raw_rpc::<ReprMockTransaction>("debug_dump_cancel_challenge_tx", params)
            .map(Into::into)
    }

    fn rpc<SuccessResponse: serde::de::DeserializeOwned>(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<SuccessResponse, RpcClientError> {
        let method_name = format!("gw_{}", method);
        self.raw_rpc(&method_name, params.clone())
            .map_err(|e| RpcClientError::ConnectionError(format!("{}({})", method, params), e))
    }

    fn raw_rpc<SuccessResponse: serde::de::DeserializeOwned>(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<SuccessResponse> {
        let mut rng = rand::thread_rng();
        let id = rng.gen_range(0..u16::MAX);

        let mut req_json = serde_json::Map::new();
        req_json.insert("id".to_owned(), serde_json::to_value(id).unwrap());
        req_json.insert("jsonrpc".to_owned(), serde_json::to_value(&"2.0").unwrap());
        req_json.insert("method".to_owned(), serde_json::to_value(method).unwrap());
        req_json.insert("params".to_owned(), params);

        let resp = self.client.post(self.url.clone()).json(&req_json).send()?;
        let output = resp.json::<jsonrpc_core::response::Output>()?;
        match output {
            jsonrpc_core::response::Output::Success(success) => {
                serde_json::from_value(success.result).map_err(|err| anyhow!(err))
            }
            jsonrpc_core::response::Output::Failure(failure) => Err(anyhow!(failure.error)),
        }
    }
}
