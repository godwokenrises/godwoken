use ckb_jsonrpc_types::Script;
use ckb_types::H256;
use gw_jsonrpc_types::{
    ckb_jsonrpc_types::{JsonBytes, Uint128, Uint32},
    debugger::{DumpChallengeTarget, ReprMockTransaction},
    godwoken::{RunResult, TxReceipt},
};
use std::{u128, u32};

type AccountID = Uint32;

pub struct GodwokenRpcClient {
    url: reqwest::Url,
    client: reqwest::blocking::Client,
    id: u64,
}

impl GodwokenRpcClient {
    pub fn new(url: &str) -> GodwokenRpcClient {
        let url = reqwest::Url::parse(url).expect("godwoken uri, e.g. \"http://127.0.0.1:8119\"");
        GodwokenRpcClient {
            url,
            id: 0,
            client: reqwest::blocking::Client::new(),
        }
    }
}

impl GodwokenRpcClient {
    pub fn get_tip_block_hash(&mut self) -> Result<Option<H256>, String> {
        let params = serde_json::Value::Null;
        self.rpc::<Option<H256>>("get_tip_block_hash", params)
            .map(|opt| opt.map(Into::into))
    }

    pub fn get_balance(&mut self, short_address: JsonBytes, sudt_id: u32) -> Result<u128, String> {
        let params = serde_json::to_value((short_address, AccountID::from(sudt_id)))
            .map_err(|err| err.to_string())?;
        self.rpc::<Uint128>("get_balance", params).map(Into::into)
    }

    pub fn get_account_id_by_script_hash(
        &mut self,
        script_hash: H256,
    ) -> Result<Option<u32>, String> {
        let params = serde_json::to_value((script_hash,)).map_err(|err| err.to_string())?;
        self.rpc::<Option<Uint32>>("get_account_id_by_script_hash", params)
            .map(|opt| opt.map(Into::into))
    }

    pub fn get_nonce(&mut self, account_id: u32) -> Result<u32, String> {
        let params =
            serde_json::to_value((AccountID::from(account_id),)).map_err(|err| err.to_string())?;
        self.rpc::<Uint32>("get_nonce", params).map(Into::into)
    }

    pub fn submit_withdrawal_request(
        &mut self,
        withdrawal_request: JsonBytes,
    ) -> Result<(), String> {
        let params = serde_json::to_value((withdrawal_request,)).map_err(|err| err.to_string())?;
        self.rpc::<()>("submit_withdrawal_request", params)
            .map(Into::into)
    }

    pub fn get_script_hash(&mut self, account_id: u32) -> Result<H256, String> {
        let params =
            serde_json::to_value((AccountID::from(account_id),)).map_err(|err| err.to_string())?;
        self.rpc::<H256>("get_script_hash", params).map(Into::into)
    }

    pub fn get_script(&mut self, script_hash: H256) -> Result<Option<Script>, String> {
        let params = serde_json::to_value((script_hash,)).map_err(|err| err.to_string())?;
        self.rpc::<Option<Script>>("get_script", params)
            .map(|opt| opt.map(Into::into))
    }

    pub fn get_script_hash_by_short_address(
        &mut self,
        short_address: JsonBytes,
    ) -> Result<Option<H256>, String> {
        let params = serde_json::to_value((short_address,)).map_err(|err| err.to_string())?;

        self.rpc::<Option<H256>>("get_script_hash_by_short_address", params)
            .map(|opt| opt.map(Into::into))
    }

    pub fn submit_l2transaction(&mut self, l2tx: JsonBytes) -> Result<H256, String> {
        let params = serde_json::to_value((l2tx,)).map_err(|err| err.to_string())?;
        self.rpc::<H256>("submit_l2transaction", params)
            .map(Into::into)
    }

    pub fn execute_l2transaction(&mut self, l2tx: JsonBytes) -> Result<RunResult, String> {
        let params = serde_json::to_value((l2tx,)).map_err(|err| err.to_string())?;
        self.rpc::<RunResult>("execute_l2transaction", params)
            .map(Into::into)
    }

    pub fn execute_raw_l2transaction(&mut self, raw_l2tx: JsonBytes) -> Result<RunResult, String> {
        let params = serde_json::to_value((raw_l2tx,)).map_err(|err| err.to_string())?;
        self.rpc::<RunResult>("execute_raw_l2transaction", params)
            .map(Into::into)
    }

    pub fn get_transaction_receipt(&mut self, tx_hash: &H256) -> Result<Option<TxReceipt>, String> {
        let params = serde_json::to_value((tx_hash,)).map_err(|err| err.to_string())?;
        self.rpc::<Option<TxReceipt>>("get_transaction_receipt", params)
            .map(|opt| opt.map(Into::into))
    }

    pub fn debug_dump_cancel_challenge_tx(
        &mut self,
        challenge_target: DumpChallengeTarget,
    ) -> Result<ReprMockTransaction, String> {
        let params = serde_json::to_value((challenge_target,)).map_err(|err| err.to_string())?;
        self.raw_rpc::<ReprMockTransaction>("debug_dump_cancel_challenge_tx", params)
            .map(Into::into)
    }

    fn rpc<SuccessResponse: serde::de::DeserializeOwned>(
        &mut self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<SuccessResponse, String> {
        let method_name = format!("gw_{}", method);
        self.raw_rpc(&method_name, params)
    }

    fn raw_rpc<SuccessResponse: serde::de::DeserializeOwned>(
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
            .map_err(|err| err.to_string())?;
        let output = resp
            .json::<ckb_jsonrpc_types::response::Output>()
            .map_err(|err| err.to_string())?;
        match output {
            ckb_jsonrpc_types::response::Output::Success(success) => {
                serde_json::from_value(success.result).map_err(|err| err.to_string())
            }
            ckb_jsonrpc_types::response::Output::Failure(failure) => Err(failure.error.to_string()),
        }
    }
}
