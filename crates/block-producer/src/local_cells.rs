//! Manage payment cells

use std::collections::HashSet;

use gw_store::{traits::chain_store::ChainStore, Store};
use gw_types::{
    core::ScriptHashType,
    offchain::CellInfo,
    packed::{OutPoint, Script},
    prelude::*,
};
use tracing::instrument;

/// Manage local consumed / live cells.
///
/// Primarily for payment cells and stake cells.
pub struct LocalCellsManager {
    store: Store,
    wallet_lock: Script,
    stake_script_type_hash: [u8; 32],
    local_consumed: HashSet<OutPoint>,
    local_live_payment: Vec<CellInfo>,
    local_live_stake: Vec<CellInfo>,
}

impl LocalCellsManager {
    pub fn create(store: Store, wallet_lock: Script, stake_script_type_hash: [u8; 32]) -> Self {
        Self {
            store,
            wallet_lock,
            stake_script_type_hash,
            local_consumed: HashSet::new(),
            local_live_payment: Vec::new(),
            local_live_stake: Vec::new(),
        }
    }

    pub fn local_consumed(&self) -> &HashSet<OutPoint> {
        &self.local_consumed
    }

    pub fn local_live_payment(&self) -> &[CellInfo] {
        &self.local_live_payment
    }

    pub fn local_live_stake(&self) -> &[CellInfo] {
        &self.local_live_stake
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
        self.local_consumed.clear();
        self.local_live_payment.clear();
        self.local_live_stake.clear();
        for b in first_unconfirmed..=last_submitted {
            let submit_tx = snap.get_submit_tx(b).expect("submit tx");
            for input in submit_tx.raw().inputs() {
                self.local_consumed.insert(input.previous_output());
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
                        self.local_live_payment.push(CellInfo {
                            out_point: OutPoint::new_builder()
                                .tx_hash(submit_tx.hash().pack())
                                .index((idx as u32).pack())
                                .build(),
                            output,
                            data: Default::default(),
                        });
                    } else if (
                        output.lock().code_hash().as_slice(),
                        output.lock().hash_type(),
                    ) == (&self.stake_script_type_hash, ScriptHashType::Type.into())
                    {
                        self.local_live_stake.push(CellInfo {
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
