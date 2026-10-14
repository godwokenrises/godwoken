//! DB-only reconstruction. Historical reports are regression references only.
mod authority;
pub mod attribution;
pub mod evm;
pub mod reports;
pub mod state;
pub mod v0;
pub mod v1;
mod v1_disposition;
pub mod v1_reports;
mod v1_state;
pub mod v1_ledgers;
pub mod v1_lending;
pub mod v1_closeout;
pub mod v1_events;
pub mod v1_balancer;
pub mod v1_wrappers;

use crate::settlement::adapter::Protocol;
use anyhow::{ensure, Result};
use serde_json::Value;
use std::path::Path;

/// Development pipeline: only the DB is a runtime input. The returned candidate
/// is explicitly incomplete and must not be published or used as a payout plan.
/// A complete public generator will be exposed after all acceptance gates pass.
pub fn reconstruct_candidate(protocol: Protocol, db: &Path) -> Result<Value> {
    ensure!(
        protocol == Protocol::V0,
        "V1 economic adapters are not complete"
    );
    let native = crate::asset_query::l2_db(protocol, db, None)?;
    let facts = state::extract(protocol, db)?;
    v0::reconstruct(&native, &facts)
}
