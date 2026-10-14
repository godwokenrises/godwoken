//! Replace only simulation authorities using the public VM fixture. No live-chain writes.
use anyhow::{ensure, Result};
use godwoken_withdraw::{
    model::*,
    settlement::adapter::{self, Protocol},
    withdraw,
};
fn main() -> Result<()> {
    let a = std::env::args().skip(1).collect::<Vec<_>>();
    ensure!(
        a.len() == 3,
        "Usage: prepare_v0_rehearsal FIXTURE_PROFILE SNAPSHOT NEW_OUTPUT_DIRECTORY"
    );
    let raw = std::fs::read(&a[0])?;
    let p = exit_withdraw_state_validator::v0::decode_profile(&raw, hash(&raw))
        .map_err(|e| anyhow::anyhow!("{e:?}"))?;
    let mut b = adapter::bundle(
        serde_json::from_slice(&std::fs::read(&a[1])?)?,
        Protocol::V0,
    )?;
    b.rollup_cell.output.lock = Script::from_packed(p.settlement.base.bootstrap_lock.clone())?;
    let dir = std::path::Path::new(&a[2]);
    ensure!(!dir.exists(), "Output exists");
    std::fs::create_dir_all(dir)?;
    let cells = p
        .auxiliary
        .iter()
        .map(|a| {
            Ok(Cell {
                out_point: withdraw::from_outpoint(&a.outpoint),
                output: withdraw::cell_output(&a.before)?,
                output_data: hx(&a.before.data),
                block_number: String::new(),
                tx_index: String::new(),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    for (name, value) in [
        ("snapshot.json", serde_json::to_value(b)?),
        ("auxiliary.json", serde_json::to_value(cells)?),
        (
            "authority.json",
            serde_json::to_value(Script::from_packed(p.settlement.base.governance.clone())?)?,
        ),
    ] {
        std::fs::write(dir.join(name), serde_json::to_vec_pretty(&value)?)?;
    }
    std::fs::write(dir.join("README.txt"),"SIMULATION ONLY: public test-key PoA and fee authorities. Actual DB root and accounting retained. Never deploy this profile or use these fake auxiliary inputs on mainnet.\n")?;
    Ok(())
}
