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

#[derive(Debug, Clone, Default)]
pub struct CollectedCustodianCells {
    pub cells_info: Vec<CellInfo>,
    pub capacity: u128,
    pub sudt: HashMap<[u8; 32], (u128, Script)>,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct WithdrawalsAmount {
    pub capacity: u128,
    pub sudt: HashMap<[u8; 32], u128>,
}

impl WithdrawalsAmount {
    pub fn is_zero(&self) -> bool {
        0 == self.capacity && self.sudt.is_empty()
    }
}

#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum TxStatus {
    /// Status "pending". The transaction is in the pool, and not proposed yet.
    Pending,
    /// Status "proposed". The transaction is in the pool and has been proposed.
    Proposed,
    /// Status "committed". The transaction has been committed to the canonical chain.
    Committed,
    /// Status "unknown". The node has not seen the transaction,
    /// or it should be rejected but was cleared due to storage limitations.
    Unknown,
    /// Status "rejected". The transaction has been recently removed from the pool.
    /// Due to storage limitations, the node can only hold the most recently removed transactions.
    Rejected,
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
