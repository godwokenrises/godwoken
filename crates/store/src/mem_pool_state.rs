use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use arc_swap::ArcSwap;
use gw_types::packed::{self, BlockInfo};

use crate::state::MemStateDB;

pub const META_MEM_BLOCK_INFO: &[u8] = b"MEM_BLOCK_INFO";
/// account SMT root
pub const META_MEM_SMT_ROOT_KEY: &[u8] = b"MEM_ACCOUNT_SMT_ROOT_KEY";
/// account SMT count
pub const META_MEM_SMT_COUNT_KEY: &[u8] = b"MEM_ACCOUNT_SMT_COUNT_KEY";

#[derive(Clone)]
pub struct Shared {
    pub state_db: MemStateDB,
    pub mem_block: Option<BlockInfo>,
}

pub struct MemPoolState {
    inner: ArcSwap<Shared>,
    completed_initial_syncing: AtomicBool,
}

impl MemPoolState {
    pub fn new(state_db: MemStateDB, completed_initial_syncing: bool) -> Self {
        Self {
            inner: ArcSwap::new(Arc::new(Shared {
                state_db,
                mem_block: None,
            })),
            completed_initial_syncing: AtomicBool::new(completed_initial_syncing),
        }
    }

    /// Create a snapshot of the current state.
    ///
    /// Each `MemStore` loaded will be independent — updates on one `MemStore`
    /// won't be seen by other `MemStore`s.
    ///
    /// Note that updates will not be stored in `MemPoolState` unless you call
    /// [`store`].
    pub fn load_state_db(&self) -> MemStateDB {
        MemStateDB::clone(&self.inner.load().state_db)
    }

    /// Replaces the snapshot inside this instance.
    pub fn store_state_db(&self, state_db: MemStateDB) {
        let mut shared = self.load_shared();
        shared.state_db = state_db;
        self.store_shared(Arc::new(shared))
    }

    pub fn get_mem_pool_block_info(&self) -> Option<packed::BlockInfo> {
        self.inner.load().mem_block.clone()
    }

    /// Load shared
    pub fn load_shared(&self) -> Shared {
        Shared::clone(&self.inner.load())
    }

    /// Store shared
    pub fn store_shared(&self, shared: Arc<Shared>) {
        self.inner.store(shared);
    }

    pub fn completed_initial_syncing(&self) -> bool {
        self.completed_initial_syncing.load(Ordering::SeqCst)
    }

    pub fn set_completed_initial_syncing(&self) {
        self.completed_initial_syncing.store(true, Ordering::SeqCst);
    }
}
