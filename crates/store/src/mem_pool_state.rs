use std::sync::Arc;

use crate::snapshot::StoreSnapshot;
use arc_swap::{ArcSwap, Guard};

pub struct MemPoolState {
    store: ArcSwap<StoreSnapshot>,
}

impl MemPoolState {
    pub fn new(store: Arc<StoreSnapshot>) -> Self {
        Self {
            store: ArcSwap::new(store),
        }
    }

    /// Provides a temporary borrow of snapshot
    pub fn load(&self) -> Guard<Arc<StoreSnapshot>> {
        self.store.load()
    }

    /// Replaces the snapshot inside this instance.
    pub fn store(&self, snapshot: Arc<StoreSnapshot>) {
        self.store.store(snapshot);
    }
}
