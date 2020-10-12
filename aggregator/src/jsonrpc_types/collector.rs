use ckb_jsonrpc_types::{JsonBytes, Script, Uint32, Uint64};
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub struct QueryParam {
    pub lock: Option<Script>,
    pub type_: Option<Script>,
    pub args_len: Option<Uint32>,
    pub data: Option<JsonBytes>,
    pub from_block: Option<Uint64>,
}
