//! Manage payment cells

use std::collections::HashSet;

use gw_store::{traits::chain_store::ChainStore, Store};
use gw_types::{
    offchain::CellInfo,
    packed::{OutPoint, Script},
    prelude::*,
};
use tracing::instrument;

/// Manage local spent / live payment cells.
pub struct PaymentCellsManager {
    store: Store,
    wallet_lock: Script,
    local_spent: HashSet<OutPoint>,
    local_live: Vec<CellInfo>,
}

impl PaymentCellsManager {
    pub fn create(store: Store, wallet_lock: Script) -> Self {
        Self {
            store,
            wallet_lock,
            local_spent: HashSet::new(),
            local_live: Vec::new(),
        }
    }

    pub fn local_spent(&self) -> &HashSet<OutPoint> {
        &self.local_spent
    }

    pub fn local_live(&self) -> &[CellInfo] {
        &self.local_live
    }

    // TODO: perf: partial update when submitted/confirmed transactions change.

    #[instrument(skip(self))]
    pub fn refresh(&mut self) {
        let snap = self.store.get_snapshot();
        let first_unconfirmed = snap
            .get_last_confirmed_block_number_hash()
            .expect("last confirmed")
            .number()
            .unpack()
            + 1;
        let last_submitted = snap
            .get_last_submitted_block_number_hash()
            .expect("last submitted")
            .number()
            .unpack();
        self.local_spent.clear();
        self.local_live.clear();
        for b in first_unconfirmed..=last_submitted {
            let submit_tx = snap.get_submit_tx(b).expect("submit tx");
            for input in submit_tx.raw().inputs() {
                self.local_spent.insert(input.previous_output());
            }
            if b == last_submitted {
                for (idx, (output, output_data)) in submit_tx
                    .raw()
                    .outputs()
                    .into_iter()
                    .zip(submit_tx.raw().outputs_data())
                    .enumerate()
                {
                    if output_data.is_empty()
                        && output.type_().is_none()
                        && output.lock() == self.wallet_lock
                    {
                        self.local_live.push(CellInfo {
                            out_point: OutPoint::new_builder()
                                .tx_hash(submit_tx.hash().pack())
                                .index((idx as u32).pack())
                                .build(),
                            output,
                            data: Default::default(),
                        });
                    }
                }
            }
        }
    }
}
