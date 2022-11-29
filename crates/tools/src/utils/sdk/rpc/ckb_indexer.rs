//! Simple http rpc client for [ckb-indexer](https://github.com/nervosnetwork/ckb-indexer)

use ckb_jsonrpc_types::{
    BlockNumber, Capacity, CellOutput, JsonBytes, OutPoint, Script, Uint32, Uint64,
};
use ckb_types::H256;
use serde::{Deserialize, Serialize};

use crate::utils::sdk::traits::{CellQueryOptions, LiveCell, PrimaryScriptType, ValueRangeOption};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SearchKey {
    pub script: Script,
    pub script_type: ScriptType,
    pub filter: Option<SearchKeyFilter>,
    pub with_data: Option<bool>,
    pub group_by_transaction: Option<bool>,
}

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
pub struct SearchKeyFilter {
    pub script: Option<Script>,
    pub script_len_range: Option<[Uint64; 2]>,
    pub output_data_len_range: Option<[Uint64; 2]>,
    pub output_capacity_range: Option<[Uint64; 2]>,
    pub block_range: Option<[BlockNumber; 2]>,
}
impl From<CellQueryOptions> for SearchKey {
    fn from(opts: CellQueryOptions) -> SearchKey {
        let convert_range =
            |range: ValueRangeOption| [Uint64::from(range.start), Uint64::from(range.end)];
        let filter = if opts.secondary_script.is_none()
            && opts.secondary_script_len_range.is_none()
            && opts.data_len_range.is_none()
            && opts.capacity_range.is_none()
            && opts.block_range.is_none()
        {
            None
        } else {
            Some(SearchKeyFilter {
                script: opts.secondary_script.map(|v| v.into()),
                script_len_range: opts.secondary_script_len_range.map(convert_range),
                output_data_len_range: opts.data_len_range.map(convert_range),
                output_capacity_range: opts.capacity_range.map(convert_range),
                block_range: opts.block_range.map(convert_range),
            })
        };
        SearchKey {
            script: opts.primary_script.into(),
            script_type: opts.primary_type.into(),
            filter,
            with_data: opts.with_data,
            group_by_transaction: None,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "snake_case")]
pub enum ScriptType {
    Lock,
    Type,
}
impl From<PrimaryScriptType> for ScriptType {
    fn from(t: PrimaryScriptType) -> ScriptType {
        match t {
            PrimaryScriptType::Lock => ScriptType::Lock,
            PrimaryScriptType::Type => ScriptType::Type,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "snake_case")]
pub enum Order {
    Desc,
    Asc,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Tip {
    pub block_hash: H256,
    pub block_number: BlockNumber,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CellsCapacity {
    pub capacity: Capacity,
    pub block_hash: H256,
    pub block_number: BlockNumber,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Cell {
    pub output: CellOutput,
    pub output_data: Option<JsonBytes>,
    pub out_point: OutPoint,
    pub block_number: BlockNumber,
    pub tx_index: Uint32,
}
impl From<Cell> for LiveCell {
    fn from(cell: Cell) -> LiveCell {
        LiveCell {
            output: cell.output.into(),
            output_data: cell
                .output_data
                .map(|data| data.into_bytes())
                .unwrap_or_default(),
            out_point: cell.out_point.into(),
            block_number: cell.block_number.value(),
            tx_index: cell.tx_index.value(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(untagged)]
pub enum Tx {
    Ungrouped(TxWithCell),
    Grouped(TxWithCells),
}

impl Tx {
    pub fn tx_hash(&self) -> H256 {
        match self {
            Tx::Ungrouped(tx) => tx.tx_hash.clone(),
            Tx::Grouped(tx) => tx.tx_hash.clone(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TxWithCell {
    pub tx_hash: H256,
    pub block_number: BlockNumber,
    pub tx_index: Uint32,
    pub io_index: Uint32,
    pub io_type: CellType,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TxWithCells {
    pub tx_hash: H256,
    pub block_number: BlockNumber,
    pub tx_index: Uint32,
    pub cells: Vec<(CellType, Uint32)>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "snake_case")]
pub enum CellType {
    Input,
    Output,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "snake_case")]
pub enum IOType {
    Input,
    Output,
}

#[derive(Serialize, Deserialize)]
pub struct Pagination<T> {
    pub objects: Vec<T>,
    pub last_cursor: JsonBytes,
}

crate::jsonrpc!(pub struct IndexerRpcClient {
    pub fn get_indexer_tip(&mut self) -> Option<Tip>;
    pub fn get_cells(&mut self, search_key: SearchKey, order: Order, limit: Uint32, after: Option<JsonBytes>) -> Pagination<Cell>;
    pub fn get_transactions(&mut self, search_key: SearchKey, order: Order, limit: Uint32, after: Option<JsonBytes>) -> Pagination<Tx>;
    pub fn get_cells_capacity(&mut self, search_key: SearchKey) -> Option<CellsCapacity>;
});
