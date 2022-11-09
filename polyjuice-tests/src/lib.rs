pub mod constant;
pub mod ctx;
#[allow(clippy::too_many_arguments)]
#[allow(dead_code)]
pub mod helper;

#[cfg(test)]
#[allow(clippy::too_many_arguments)]
pub(crate) mod test_cases;

use gw_common::{smt::SMT, H256};
pub use gw_store;
use gw_store::{
    smt::smt_store::SMTStateStore,
    snapshot::StoreSnapshot,
    state::{
        overlay::{mem_state::MemStateTree, mem_store::MemStore},
        MemStateDB,
    },
};
pub use gw_types;

type DummyState = MemStateDB;
pub fn new_dummy_state(store: StoreSnapshot) -> MemStateDB {
    let smt = SMT::new(H256::zero(), SMTStateStore::new(MemStore::new(store)));
    let inner = MemStateTree::new(smt, 0);
    MemStateDB::new(inner)
}
