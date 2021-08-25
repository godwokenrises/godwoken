use crate::godwoken::ChallengeTargetType;

use ckb_fixed_hash::H256 as JsonH256;
use ckb_jsonrpc_types as json_types;
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
pub struct ReprMockCellDep {
    pub cell_dep: json_types::CellDep,
    pub output: json_types::CellOutput,
    pub data: json_types::JsonBytes,
    pub header: Option<JsonH256>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct ReprMockInput {
    pub input: json_types::CellInput,
    pub output: json_types::CellOutput,
    pub data: json_types::JsonBytes,
    pub header: Option<JsonH256>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct ReprMockInfo {
    pub inputs: Vec<ReprMockInput>,
    pub cell_deps: Vec<ReprMockCellDep>,
    pub header_deps: Vec<json_types::HeaderView>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct ReprMockTransaction {
    pub mock_info: ReprMockInfo,
    pub tx: json_types::Transaction,
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DumpChallengeTarget {
    ByBlockHash {
        block_hash: JsonH256,
        target_index: json_types::Uint32,
        target_type: ChallengeTargetType,
    },
    ByBlockNumber {
        block_number: json_types::Uint64,
        target_index: json_types::Uint32,
        target_type: ChallengeTargetType,
    },
}
