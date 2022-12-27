use std::time::Duration;

use async_jsonrpc_client::Output;
use gw_types::h256::H256;
use serde::de::DeserializeOwned;
use serde_json::from_value;

pub(crate) const DEFAULT_QUERY_LIMIT: usize = 500;
pub(crate) const DEFAULT_HTTP_TIMEOUT: Duration = Duration::from_secs(15);

pub(crate) type JsonH256 = ckb_fixed_hash::H256;

pub(crate) fn to_h256(v: JsonH256) -> H256 {
    v.into()
}

pub(crate) fn to_jsonh256(v: H256) -> JsonH256 {
    v.into()
}

pub(crate) fn to_result<T: DeserializeOwned>(output: Output) -> anyhow::Result<T> {
    match output {
        Output::Success(success) => Ok(from_value(success.result)?),
        Output::Failure(failure) => Err(failure.error.into()),
    }
}
