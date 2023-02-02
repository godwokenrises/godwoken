use std::collections::HashMap;

use crate::{
    bytes::Bytes,
    packed::{CellInput, CellOutput, DepositRequest, OutPoint, Script},
    prelude::*,
};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CellInfo {
    pub out_point: OutPoint,
    pub output: CellOutput,
    pub data: Bytes,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CellStatus {
    Live,
    Dead,
    Unknown,
}

impl Default for CellStatus {
    fn default() -> Self {
        Self::Unknown
    }
}

#[derive(Debug, Clone, Default)]
pub struct CellWithStatus {
    pub cell: Option<CellInfo>,
    pub status: CellStatus,
}

#[derive(Debug, Clone)]
pub struct InputCellInfo {
    pub input: CellInput,
    pub cell: CellInfo,
}

impl From<CellInfo> for InputCellInfo {
    fn from(cell: CellInfo) -> Self {
        Self {
            input: CellInput::new_builder()
                .previous_output(cell.out_point.clone())
                .build(),
            cell,
        }
    }
}

impl InputCellInfo {
    pub fn with_since(cell: CellInfo, since: u64) -> Self {
        Self {
            input: CellInput::new_builder()
                .previous_output(cell.out_point.clone())
                .since(since.pack())
                .build(),
            cell,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct CollectedCustodianCells {
    pub cells_info: Vec<CellInfo>,
    pub capacity: u128,
    pub sudt: HashMap<[u8; 32], (u128, Script)>,
}

#[derive(Debug, Default)]
pub struct WithdrawalsAmount {
    pub capacity: u128,
    pub sudt: HashMap<[u8; 32], u128>,
}

#[derive(Debug, Clone, Default)]
pub struct DepositInfo {
    pub request: DepositRequest,
    pub cell: CellInfo,
}

#[derive(Debug, Clone, Default)]
pub struct SUDTStat {
    pub total_amount: u128,
    pub finalized_amount: u128,
    pub cells_count: usize,
}

#[derive(Debug, Clone)]
pub struct CustodianStat {
    pub total_capacity: u128,
    pub finalized_capacity: u128,
    pub cells_count: usize,
    pub ckb_cells_count: usize,
    pub sudt_stat: HashMap<ckb_types::packed::Script, SUDTStat>,
}
