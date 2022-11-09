use std::convert::TryFrom;

use ckb_fixed_hash::H256 as JsonH256;
use ckb_jsonrpc_types::{JsonBytes, Uint64};
use gw_types::offchain::{self};
use serde::{Deserialize, Serialize};

use crate::godwoken::LogItem;

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub struct CycleMeter {
    pub execution: Uint64,
    pub r#virtual: Uint64,
    pub total: Uint64,
}

impl From<offchain::CycleMeter> for CycleMeter {
    fn from(m: offchain::CycleMeter) -> Self {
        Self {
            execution: m.execution.into(),
            r#virtual: m.r#virtual.into(),
            total: m.total().into(),
        }
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub struct DebugRunResult {
    // return data
    pub return_data: JsonBytes,
    // log data
    pub logs: Vec<LogItem>,
    pub exit_code: i8,
    pub cycles: CycleMeter,
    pub read_data_hashes: Vec<JsonH256>,
    pub write_data_hashes: Vec<JsonH256>,
    pub debug_log: Vec<String>,
}

impl TryFrom<offchain::RunResult> for DebugRunResult {
    type Error = anyhow::Error;
    fn try_from(data: offchain::RunResult) -> Result<DebugRunResult, Self::Error> {
        let offchain::RunResult {
            return_data,
            logs,
            exit_code,
            cycles,
            read_data_hashes,
            write_data_hashes,
            debug_log_buf,
            ..
        } = data;
        Ok(DebugRunResult {
            return_data: JsonBytes::from_bytes(return_data),
            logs: logs.into_iter().map(Into::into).collect(),
            exit_code,
            cycles: cycles.into(),
            read_data_hashes: read_data_hashes
                .into_iter()
                .map(|h| JsonH256::from_slice(h.as_slice()).unwrap())
                .collect(),
            write_data_hashes: write_data_hashes
                .into_iter()
                .map(|h| JsonH256::from_slice(h.as_slice()).unwrap())
                .collect(),
            debug_log: String::from_utf8(debug_log_buf)?
                .lines()
                .map(ToString::to_string)
                .collect(),
        })
    }
}
