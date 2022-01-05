use thiserror::Error;

// NOTE: Error only for [client].request() not to_result()
#[derive(Error, Debug)]
#[error("{client} error, method: {method} error: {source}")]
pub struct RPCRequestError {
    pub client: &'static str,
    pub method: String,
    pub source: anyhow::Error,
}

impl RPCRequestError {
    pub fn new<E: Into<anyhow::Error>>(client: &'static str, method: String, source: E) -> Self {
        RPCRequestError {
            client,
            method,
            source: source.into(),
        }
    }
}
