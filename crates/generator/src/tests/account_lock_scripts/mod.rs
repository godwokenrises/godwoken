mod eth_account_lock;

use ckb_traits::{CellDataProvider, HeaderProvider};
use ckb_types::{
    bytes::Bytes,
    core::{EpochExt, HeaderView},
    packed::{Byte32, CellOutput, OutPoint},
};
use lazy_static::lazy_static;
use std::collections::HashMap;

pub const MAX_CYCLES: u64 = std::u64::MAX;

lazy_static! {
    pub static ref SECP256K1_DATA_BIN: Bytes = Bytes::from(
        &include_bytes!("../../../../../c/deps/ckb-miscellaneous-scripts/build/secp256k1_data")[..]
    );
}

#[derive(Default)]
pub struct DummyDataLoader {
    pub cells: HashMap<OutPoint, (CellOutput, Bytes)>,
    pub headers: HashMap<Byte32, HeaderView>,
    pub epoches: HashMap<Byte32, EpochExt>,
}

impl DummyDataLoader {
    fn new() -> Self {
        Self::default()
    }
}

impl CellDataProvider for DummyDataLoader {
    fn get_cell_data(&self, out_point: &OutPoint) -> Option<(Bytes, Byte32)> {
        self.cells
            .get(&out_point)
            .map(|(_, data)| (data.clone(), CellOutput::calc_data_hash(&data)))
    }
}

impl HeaderProvider for DummyDataLoader {
    // load header
    fn get_header(&self, block_hash: &Byte32) -> Option<HeaderView> {
        self.headers.get(block_hash).cloned()
    }
}
