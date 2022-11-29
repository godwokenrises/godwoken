mod ckb;
pub mod ckb_indexer;
pub mod ckb_light_client;

pub use ckb::CkbRpcClient;
pub use ckb_indexer::IndexerRpcClient;
pub use ckb_light_client::LightClientRpcClient;

use thiserror::Error;

#[derive(Error, Debug)]
pub enum RpcError {
    #[error("parse json error: `{0}`")]
    Json(#[from] serde_json::Error),
    #[error("http error: `{0}`")]
    Http(#[from] reqwest::Error),
    #[error("jsonrpc error: `{0}`")]
    Rpc(#[from] jsonrpc_core::Error),
}

#[macro_export]
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
                let url = reqwest::Url::parse(uri).expect("ckb uri, e.g. \"http://127.0.0.1:8114\"");
                $struct_name { url, id: 0, client: reqwest::blocking::Client::new(), }
            }

            $(
                $(#[$attr])*
                pub fn $method(&mut $selff $(, $arg_name: $arg_ty)*) -> Result<$return_ty, $crate::utils::sdk::rpc::RpcError> {
                    let method = String::from(stringify!($method));
                    let params = $crate::serialize_parameters!($($arg_name,)*);
                    $selff.id += 1;

                    let mut req_json = serde_json::Map::new();
                    req_json.insert("id".to_owned(), serde_json::json!($selff.id));
                    req_json.insert("jsonrpc".to_owned(), serde_json::json!("2.0"));
                    req_json.insert("method".to_owned(), serde_json::json!(method));
                    req_json.insert("params".to_owned(), params);

                    let resp = $selff.client.post($selff.url.clone()).json(&req_json).send()?;
                    let output = resp.json::<jsonrpc_core::response::Output>()?;
                    match output {
                        jsonrpc_core::response::Output::Success(success) => {
                            serde_json::from_value(success.result).map_err(Into::into)
                        },
                        jsonrpc_core::response::Output::Failure(failure) => {
                            Err(failure.error.into())
                        }
                    }
                }
            )*
        }
    )
}

#[macro_export]
macro_rules! serialize_parameters {
    () => ( serde_json::Value::Null );
    ($($arg_name:ident,)+) => ( serde_json::to_value(($($arg_name,)+))?)
}

#[cfg(test)]
mod anyhow_tests {
    use anyhow::anyhow;
    #[test]
    fn test_rpc_error() {
        let json_rpc_error = jsonrpc_core::Error {
            code: jsonrpc_core::ErrorCode::ParseError,
            message: "parse error".to_string(),
            data: None,
        };
        let error = super::RpcError::from(json_rpc_error);
        let error = anyhow!(error);
        println!("{}", error)
    }
}
