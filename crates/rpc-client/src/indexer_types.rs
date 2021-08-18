// This file contains types that are copied over from ckb-indexer project.

use ckb_fixed_hash::H256;
use gw_jsonrpc_types::ckb_jsonrpc_types::{
    BlockNumber, CellOutput, JsonBytes, OutPoint, Script, Uint32, Uint64,
};
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize)]
pub struct SearchKey {
    pub script: Script,
    pub script_type: ScriptType,
    pub filter: Option<SearchKeyFilter>,
}

#[derive(Deserialize, Serialize, Default)]
pub struct SearchKeyFilter {
    pub script: Option<Script>,
    pub output_data_len_range: Option<[Uint64; 2]>,
    pub output_capacity_range: Option<[Uint64; 2]>,
    pub block_range: Option<[BlockNumber; 2]>,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ScriptType {
    Lock,
    Type,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Order {
    Desc,
    Asc,
}

#[derive(Deserialize, Serialize)]
pub struct Tx {
    pub tx_hash: H256,
    pub block_number: BlockNumber,
    pub tx_index: Uint32,
    pub io_index: Uint32,
    pub io_type: IOType,
}

#[derive(Serialize, Deserialize)]
pub struct Cell {
    pub output: CellOutput,
    pub output_data: JsonBytes,
    pub out_point: OutPoint,
    pub block_number: BlockNumber,
    pub tx_index: Uint32,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IOType {
    Input,
    Output,
}

#[derive(Deserialize, Serialize)]
pub struct Pagination<T> {
    pub objects: Vec<T>,
    pub last_cursor: JsonBytes,
}
