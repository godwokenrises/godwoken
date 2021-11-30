use crate::packed::{DepositRequest, Script};
use crate::{
    bytes::Bytes,
    packed::{CellInput, CellOutput, OutPoint},
};
use std::collections::HashMap;

#[derive(Debug, Clone, Default)]
pub struct CellInfo {
    pub out_point: OutPoint,
    pub output: CellOutput,
    pub data: Bytes,
}

#[derive(Debug, Clone)]
pub struct InputCellInfo {
    pub input: CellInput,
    pub cell: CellInfo,
}

#[derive(Debug, Clone)]
pub struct CollectedCustodianCells {
    pub cells_info: Vec<CellInfo>,
    pub capacity: u128,
    pub sudt: HashMap<[u8; 32], (u128, Script)>,
}

impl Default for CollectedCustodianCells {
    fn default() -> Self {
        CollectedCustodianCells {
            cells_info: Default::default(),
            capacity: 0,
            sudt: Default::default(),
        }
    }
}

#[derive(Debug)]
pub struct WithdrawalsAmount {
    pub capacity: u128,
    pub sudt: HashMap<[u8; 32], u128>,
}

impl Default for WithdrawalsAmount {
    fn default() -> Self {
        WithdrawalsAmount {
            capacity: 0,
            sudt: Default::default(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum TxStatus {
    /// Status "pending". The transaction is in the pool, and not proposed yet.
    Pending,
    /// Status "proposed". The transaction is in the pool and has been proposed.
    Proposed,
    /// Status "committed". The transaction has been committed to the canonical chain.
    Committed,
}

#[derive(Debug, Clone)]
pub struct DepositInfo {
    pub request: DepositRequest,
    pub cell: CellInfo,
}

#[derive(Debug, Clone)]
pub struct CustodianStat {
    pub total_capacity: u128,
    pub cells_count: usize,
}
