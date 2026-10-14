//! Offline-signable exit transaction construction and chain-derived recovery.
pub mod build;
pub mod chain;
pub mod state;
pub mod report;
use crate::model::*;
use anyhow::{ensure, Context, Result};
use ckb_types::prelude::Entity as CkbEntity;
use exit_withdraw_state_validator as contract;
use gw_types::{packed, prelude::*};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Deployment {
    pub profile: String,
    pub profile_hash: String,
    pub validator_data_hash: String,
    pub cell_deps: Vec<Dependency>,
    pub sponsor_inputs: Vec<OutPoint>,
    /// Complete legacy multisig preimage: reserved, require_first_n, threshold, count, pubkey hashes.
    pub multisig_config: String,
    /// Required only for the bootstrap rollup input's lock signature placeholder.
    pub bootstrap_lock_placeholder: Option<String>,
    pub fee_shannons: u64,
    pub confirmations: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Dependency {
    pub out_point: OutPoint,
    pub dep_type: String,
    pub data_hash: String,
}
impl Deployment {
    pub fn profile(&self) -> Result<contract::Profile> {
        contract::codec::decode_profile(&bytes(&self.profile)?, hash32(&self.profile_hash)?)
            .map_err(|e| anyhow::anyhow!("Invalid profile: {e:?}"))
    }
    pub fn multisig_placeholder(&self, p: &contract::Profile) -> Result<Vec<u8>> {
        let c = bytes(&self.multisig_config)?;
        ensure!(
            c.len() >= 4
                && c[0] == 0
                && c[2] > 0
                && c[1] <= c[2]
                && c[2] <= c[3]
                && c.len() == 4 + 20 * c[3] as usize,
            "Invalid multisig config"
        );
        ensure!(
            p.governance.args().raw_data().as_ref() == &hash(&c)[..20],
            "Multisig config does not match governance lock"
        );
        let mut placeholder = c.clone();
        placeholder.resize(c.len() + 65 * c[2] as usize, 0);
        Ok(placeholder)
    }
}
pub fn outpoint(p: &OutPoint) -> Result<packed::OutPoint> {
    Ok(packed::OutPoint::new_builder()
        .tx_hash(hash32(&p.tx_hash)?.pack())
        .index(u32::try_from(quantity(&p.index)?)?.pack())
        .build())
}
pub fn from_outpoint(p: &packed::OutPoint) -> OutPoint {
    OutPoint {
        tx_hash: hx(p.tx_hash().as_slice()),
        index: format!(
            "0x{:x}",
            u32::from_le_bytes(p.index().as_slice().try_into().unwrap())
        ),
    }
}
pub fn contract_cell(c: &Cell) -> Result<contract::Cell> {
    output_cell(&c.output, &c.output_data)
}
pub fn output_cell(output: &CellOutput, data: &str) -> Result<contract::Cell> {
    Ok(contract::Cell {
        capacity: quantity(&output.capacity)?,
        lock: output.lock.packed()?,
        type_: output
            .type_script
            .as_ref()
            .map(Script::packed)
            .transpose()?,
        data: bytes(data)?,
    })
}
pub fn cell_output(c: &contract::Cell) -> Result<CellOutput> {
    Ok(CellOutput {
        capacity: format!("0x{:x}", c.capacity),
        lock: Script::from_packed(c.lock.clone())?,
        type_script: c
            .type_
            .as_ref()
            .map(|s| Script::from_packed(s.clone()))
            .transpose()?,
    })
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Transaction {
    pub version: String,
    pub cell_deps: Vec<serde_json::Value>,
    pub header_deps: Vec<String>,
    pub inputs: Vec<serde_json::Value>,
    pub outputs: Vec<CellOutput>,
    pub outputs_data: Vec<String>,
    pub witnesses: Vec<String>,
}
impl Transaction {
    pub fn packed(&self) -> Result<packed::Transaction> {
        let tx: ckb_jsonrpc_types::Transaction =
            serde_json::from_value(serde_json::to_value(self)?)?;
        let tx: ckb_types::packed::Transaction = tx.into();
        packed::Transaction::from_slice(tx.as_slice()).context("Transaction encoding")
    }
    pub fn hash(&self) -> Result<String> {
        Ok(hx(self.packed()?.hash()))
    }
    pub fn input_points(&self) -> Result<Vec<OutPoint>> {
        self.inputs
            .iter()
            .map(|i| serde_json::from_value(i["previous_output"].clone()).map_err(Into::into))
            .collect()
    }
    pub fn cell(&self, point: &OutPoint) -> Result<Cell> {
        ensure!(self.hash()? == point.tx_hash, "Transaction hash mismatch");
        let i = usize::try_from(quantity(&point.index)?)?;
        Ok(Cell {
            out_point: point.clone(),
            output: self.outputs.get(i).context("Missing output")?.clone(),
            output_data: self
                .outputs_data
                .get(i)
                .context("Missing output data")?
                .clone(),
            block_number: String::new(),
            tx_index: String::new(),
        })
    }
}

/// Resolve every executed script to a pinned explicit dependency before exporting.
pub(crate) fn verify_script_dependencies(
    inputs: &[Cell],
    outputs: &[CellOutput],
    deps: &[Cell],
) -> Result<()> {
    for script in inputs
        .iter()
        .map(|c| &c.output.lock)
        .chain(inputs.iter().filter_map(|c| c.output.type_script.as_ref()))
        .chain(outputs.iter().filter_map(|c| c.type_script.as_ref()))
    {
        let matches = deps
            .iter()
            .filter(|c| match script.hash_type.as_str() {
                "type" => {
                    c.output
                        .type_script
                        .as_ref()
                        .and_then(|t| t.hash().ok())
                        .as_ref()
                        == Some(&script.code_hash)
                }
                "data" | "data1" | "data2" => {
                    bytes(&c.output_data).is_ok_and(|b| hx(hash(&b)) == script.code_hash)
                }
                _ => false,
            })
            .count();
        ensure!(
            matches == 1,
            "Executed script must resolve to one pinned code dependency: {}",
            script.code_hash
        );
    }
    Ok(())
}
pub(crate) fn verify_size(tx: &Transaction) -> Result<()> {
    ensure!(
        tx.packed()?.as_slice().len() <= 512_000,
        "Transaction exceeds 512,000-byte size budget; split request"
    );
    Ok(())
}
pub fn prepare(
    db: &crate::db::Database,
    rpc: &impl chain::Chain,
    d: &Deployment,
    bundle: &Bundle,
    requests: &[build::Request],
) -> Result<build::Skeleton> {
    let profile = d.profile()?;
    let (_, cfg) = bundle.validate()?;
    let dependencies = chain::verify_dependencies(rpc, d, &profile)?;
    let (state, journal) = chain::reconstruct(
        db,
        rpc,
        &profile,
        &cfg.packed,
        &bundle.rollup_cell,
        d.confirmations,
    )?;
    let sponsors = d
        .sponsor_inputs
        .iter()
        .map(|p| chain::live(rpc, p))
        .collect::<Result<Vec<_>>>()?;
    let skeleton = build::construct(
        db,
        &state,
        d,
        bundle,
        requests,
        sponsors,
        dependencies,
        serde_json::to_value(journal)?,
    )?;
    verify_script_dependencies(
        &skeleton.input_cells,
        &skeleton.transaction.outputs,
        &skeleton.dependency_cells,
    )?;
    verify_size(&skeleton.transaction)?;
    for cell in skeleton
        .input_cells
        .iter()
        .chain(&skeleton.dependency_cells)
    {
        ensure!(
            chain::same_cell(&chain::live(rpc, &cell.out_point)?, cell),
            "Selected cell changed while building"
        );
    }
    Ok(skeleton)
}

pub fn verify_signed(
    db: &crate::db::Database,
    rpc: &impl chain::Chain,
    d: &Deployment,
    bundle: &Bundle,
    tx: &Transaction,
    max_cycles: u64,
) -> Result<serde_json::Value> {
    ensure!(
        max_cycles > 0 && max_cycles <= 70_000_000,
        "Signed transaction cycle cap must be within 1..70,000,000"
    );
    let profile = d.profile()?;
    let (_, cfg) = bundle.validate()?;
    let dependencies = chain::verify_dependencies(rpc, d, &profile)?;
    verify_size(tx)?;
    ensure!(
        tx.inputs.len() <= contract::MAX_INPUTS && tx.outputs.len() <= contract::MAX_OUTPUTS,
        "Signed exit exceeds contract bounds"
    );
    let (state, _) = chain::reconstruct(
        db,
        rpc,
        &profile,
        &cfg.packed,
        &bundle.rollup_cell,
        d.confirmations,
    )?;
    ensure!(
        tx.cell_deps
            == d.cell_deps
                .iter()
                .map(|x| serde_json::json!({"out_point":x.out_point,"dep_type":x.dep_type}))
                .collect::<Vec<_>>(),
        "Transaction dependencies differ from deployment"
    );
    let inputs = tx
        .input_points()?
        .iter()
        .map(|op| chain::live(rpc, op))
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        inputs
            .iter()
            .any(|c| chain::same_cell(c, &bundle.rollup_cell)),
        "Signed transaction uses a stale rollup"
    );
    verify_script_dependencies(&inputs, &tx.outputs, &dependencies)?;
    let a = chain::action(tx, &profile)?;
    for (k, v) in &a.kv {
        ensure!(
            state.read(*k)? == *v,
            "Signed witness differs from current reconstructed state"
        );
    }
    let outcome = contract::verify(&profile, &cfg.packed, &chain::resolved(tx, &inputs)?, &a)
        .map_err(|e| anyhow::anyhow!("Signed transaction violates recovery rules: {e:?}"))?;
    ensure!(
        outcome.fee == d.fee_shannons,
        "Signed fee differs from approved deployment fee"
    );
    let result = rpc.call("estimate_cycles", serde_json::json!([tx]))?;
    let cycles = quantity(
        result["cycles"]
            .as_str()
            .context("Missing full-transaction cycles")?,
    )?;
    ensure!(
        cycles <= max_cycles,
        "Full signed transaction exceeds cycle budget; split request"
    );
    for c in inputs.iter().chain(&dependencies) {
        ensure!(
            chain::same_cell(&chain::live(rpc, &c.out_point)?, c),
            "Input spent during signed validation"
        );
    }
    Ok(
        serde_json::json!({"status":"verified_not_broadcast","transaction_hash":tx.hash()?,"cycles":cycles,"max_cycles":max_cycles,"before_root":hx(state.root()),"after_root":hx(outcome.root)}),
    )
}
