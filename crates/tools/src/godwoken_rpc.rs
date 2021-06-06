use ckb_types::H256;
use gw_jsonrpc_types::ckb_jsonrpc_types::{Uint128, Uint32};
use std::{u128, u32};

type AccountID = Uint32;
type JsonH256 = ckb_fixed_hash::H256;

macro_rules! jsonrpc {
    (
        $(#[$struct_attr:meta])*
        pub struct $struct_name:ident {$(
            $(#[$attr:meta])*
            pub fn $method:ident(&mut $selff:ident $(, $arg_name:ident: $arg_ty:ty)*)
                -> $return_ty:ty;
        )*}
    ) => (
        $(#[$struct_attr])*
        pub struct $struct_name {
            pub client: reqwest::blocking::Client,
            pub url: reqwest::Url,
            pub id: u64,
        }

        impl $struct_name {
            pub fn new(uri: &str) -> Self {
                let url = reqwest::Url::parse(uri).expect("ckb uri, e.g. \"http://127.0.0.1:8119\"");
                $struct_name { url, id: 0, client: reqwest::blocking::Client::new(), }
            }

            $(
                $(#[$attr])*
                pub fn $method(&mut $selff $(, $arg_name: $arg_ty)*) -> Result<$return_ty, failure::Error> {
                    let method = String::from(stringify!($method));
                    let params = serialize_parameters!($($arg_name,)*);
                    $selff.id += 1;

                    let mut req_json = serde_json::Map::new();
                    req_json.insert("id".to_owned(), serde_json::json!($selff.id));
                    req_json.insert("jsonrpc".to_owned(), serde_json::json!("2.0"));
                    req_json.insert("method".to_owned(), serde_json::json!(method));
                    req_json.insert("params".to_owned(), params);

                    let resp = $selff.client.post($selff.url.clone()).json(&req_json).send()?;
                    let output = resp.json::<ckb_jsonrpc_types::response::Output>()?;
                    match output {
                        ckb_jsonrpc_types::response::Output::Success(success) => {
                            serde_json::from_value(success.result).map_err(Into::into)
                        },
                        ckb_jsonrpc_types::response::Output::Failure(failure) => {
                            Err(failure.error.into())
                        }
                    }
                }
            )*
        }
    )
}

macro_rules! serialize_parameters {
    () => ( serde_json::Value::Null );
    ($($arg_name:ident,)+) => ( serde_json::to_value(($($arg_name,)+))?)
}

jsonrpc!(pub struct RawGodwokenRpcClient {
    pub fn get_tip_block_hash(&mut self) -> Option<H256>;
    pub fn get_balance(&mut self, account_id: AccountID, sudt_id: AccountID) -> Uint128;
    pub fn get_account_id_by_script_hash(&mut self, script_hash: JsonH256) -> Option<AccountID>;
});

pub struct GodwokenRpcClient {
    url: String,
    client: RawGodwokenRpcClient,
}

impl GodwokenRpcClient {
    pub fn new(url: String) -> GodwokenRpcClient {
        let client = RawGodwokenRpcClient::new(url.as_str());
        GodwokenRpcClient { url, client }
    }

    pub fn url(&self) -> &str {
        self.url.as_str()
    }

    pub fn client(&mut self) -> &mut RawGodwokenRpcClient {
        &mut self.client
    }
}

impl GodwokenRpcClient {
    pub fn get_tip_block_hash(&mut self) -> Result<Option<H256>, String> {
        self.client
            .get_tip_block_hash()
            .map(|opt| opt.map(Into::into))
            .map_err(|err| err.to_string())
    }

    pub fn get_balance(&mut self, account_id: u32, sudt_id: u32) -> Result<u128, String> {
        self.client
            .get_balance(AccountID::from(account_id), AccountID::from(sudt_id))
            .map(Into::into)
            .map_err(|err| err.to_string())
    }

    pub fn get_account_id_by_script_hash(
        &mut self,
        script_hash: H256,
    ) -> Result<Option<u32>, String> {
        self.client
            .get_account_id_by_script_hash(script_hash)
            .map(|opt| opt.map(Into::into))
            .map_err(|err| err.to_string())
    }
}
