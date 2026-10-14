use super::state::RecoveryState;
use super::*;
use crate::{db::Database, network::Rpc};
use anyhow::{bail, ensure, Context, Result};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

pub trait Chain {
    fn call(&self, method: &str, params: Value) -> Result<Value>;
}
impl Chain for Rpc {
    fn call(&self, m: &str, p: Value) -> Result<Value> {
        Rpc::call(self, m, p)
    }
}
pub fn live(rpc: &impl Chain, point: &OutPoint) -> Result<Cell> {
    let v = rpc.call("get_live_cell", json!([point, true]))?;
    ensure!(
        v["status"] == "live",
        "Required input/dependency is not live: {}:{}",
        point.tx_hash,
        point.index
    );
    Ok(Cell {
        out_point: point.clone(),
        output: serde_json::from_value(v["cell"]["output"].clone())?,
        output_data: v["cell"]["data"]["content"]
            .as_str()
            .context("Missing live data")?
            .into(),
        block_number: String::new(),
        tx_index: String::new(),
    })
}
pub fn same_cell(a: &Cell, b: &Cell) -> bool {
    a.out_point == b.out_point && a.output == b.output && a.output_data == b.output_data
}
pub struct History<'a, C> {
    rpc: &'a C,
    cache: BTreeMap<String, (Arc<Transaction>, String, String)>,
    tip: u64,
    confirmations: u64,
}
impl<'a, C: Chain> History<'a, C> {
    pub fn new(rpc: &'a C, confirmations: u64) -> Result<Self> {
        ensure!(confirmations > 0, "Confirmation depth must be positive");
        let tip = rpc.call("get_tip_header", json!([]))?;
        Ok(Self {
            rpc,
            cache: BTreeMap::new(),
            tip: quantity(tip["number"].as_str().context("Missing tip number")?)?,
            confirmations,
        })
    }
    pub fn transaction(&mut self, hash: &str) -> Result<Arc<Transaction>> {
        if let Some((tx, _, _)) = self.cache.get(hash) {
            return Ok(tx.clone());
        }
        let v = self.rpc.call("get_transaction", json!([hash]))?;
        ensure!(
            v["tx_status"]["status"] == "committed",
            "Missing or uncommitted history transaction {hash}"
        );
        let block = v["tx_status"]["block_hash"]
            .as_str()
            .context("Missing block hash")?;
        let header = self.rpc.call("get_header", json!([block]))?;
        ensure!(header["hash"] == block, "Missing history header");
        let number = header["number"]
            .as_str()
            .context("Missing history block number")?;
        let n = quantity(number)?;
        ensure!(
            self.tip
                .checked_sub(n)
                .and_then(|v| v.checked_add(1))
                .is_some_and(|v| v >= self.confirmations),
            "History transaction has insufficient confirmations"
        );
        let canonical = self.rpc.call("get_header_by_number", json!([number]))?;
        ensure!(
            canonical["hash"] == block,
            "History transaction is on an orphaned block"
        );
        let tx: Transaction = serde_json::from_value(v["transaction"].clone())
            .context("Node must return historical transaction JSON including witnesses")?;
        ensure!(tx.hash()? == hash, "RPC transaction hash/preimage mismatch");
        let tx = Arc::new(tx);
        self.cache
            .insert(hash.into(), (Arc::clone(&tx), number.into(), block.into()));
        Ok(tx)
    }
    pub fn cell(&mut self, point: &OutPoint) -> Result<Cell> {
        self.transaction(&point.tx_hash)?.cell(point)
    }
    pub fn recheck(&self) -> Result<()> {
        for (hash, (_, number, block)) in &self.cache {
            let v = self.rpc.call("get_transaction", json!([hash]))?;
            let header = self.rpc.call("get_header_by_number", json!([number]))?;
            ensure!(
                v["tx_status"]["status"] == "committed"
                    && v["tx_status"]["block_hash"].as_str() == Some(block.as_str())
                    && header["hash"].as_str() == Some(block.as_str()),
                "Chain reorganized during reconstruction"
            );
        }
        Ok(())
    }
}
pub fn resolved(tx: &Transaction, inputs: &[Cell]) -> Result<contract::Transaction> {
    ensure!(
        inputs.len() == tx.inputs.len() && tx.outputs.len() == tx.outputs_data.len(),
        "Malformed transaction shape"
    );
    let points = tx.input_points()?;
    ensure!(
        points.iter().zip(inputs).all(|(p, c)| *p == c.out_point),
        "Resolved inputs differ from transaction"
    );
    Ok(contract::Transaction {
        inputs: inputs.iter().map(contract_cell).collect::<Result<_>>()?,
        outputs: tx
            .outputs
            .iter()
            .zip(&tx.outputs_data)
            .map(|(o, d)| output_cell(o, d))
            .collect::<Result<_>>()?,
        outpoints: points.iter().map(outpoint).collect::<Result<_>>()?,
    })
}
pub fn action(tx: &Transaction, profile: &contract::Profile) -> Result<contract::Action> {
    let rollup = Script::from_packed(profile.rollup.clone())?;
    let i = tx
        .outputs
        .iter()
        .position(|o| o.type_script.as_ref() == Some(&rollup))
        .context("Missing output rollup")?;
    let w = bytes(
        tx.witnesses
            .get(i)
            .context("Missing rollup output witness")?,
    )?;
    let w = packed::WitnessArgsReader::from_slice(&w)?;
    let data = w
        .output_type()
        .to_opt()
        .context("Missing recovery action")?;
    contract::codec::decode_action(data.raw_data())
        .map_err(|e| anyhow::anyhow!("Recovery action: {e:?}"))
}
#[derive(Debug, Serialize)]
pub struct Replay {
    pub transaction_hash: String,
    pub input_rollup: OutPoint,
    pub output_rollup: OutPoint,
    pub before_root: String,
    pub after_root: String,
}
pub fn reconstruct<'a>(
    db: &'a Database,
    rpc: &impl Chain,
    p: &contract::Profile,
    cfg: &packed::RollupConfig,
    current: &Cell,
    confirmations: u64,
) -> Result<(RecoveryState<'a>, Vec<Replay>)> {
    let mut state = RecoveryState::new(db)?;
    ensure!(
        p.bootstrap_state.account().merkle_root().as_slice() == state.root(),
        "DB is not the profile bootstrap snapshot"
    );
    ensure!(
        u32::from_le_bytes(p.bootstrap_state.account().count().as_slice().try_into()?)
            == db.tip.account_count
            && p.bootstrap_state.tip_block_hash().as_slice() == hash32(&db.tip.hash)?,
        "Bootstrap metadata differs from DB"
    );
    let anchor = from_outpoint(&p.bootstrap_outpoint);
    let rollup = Script::from_packed(p.rollup.clone())?;
    let mut history = History::new(rpc, confirmations)?;
    // Also authenticate the current anchor's inclusion/confirmations, even before the first exit.
    let observed = history.cell(&current.out_point)?;
    ensure!(
        same_cell(&observed, current),
        "Current rollup differs from canonical history"
    );
    let mut cursor = current.out_point.clone();
    let mut seen = BTreeSet::new();
    let mut chain = Vec::new();
    while cursor != anchor {
        ensure!(
            chain.len() < 10_000 && seen.insert(cursor.clone()),
            "Recovery history limit or cycle"
        );
        let tx = history.transaction(&cursor.tx_hash)?;
        ensure!(
            tx.inputs.len() <= contract::MAX_INPUTS && tx.outputs.len() <= contract::MAX_OUTPUTS,
            "Historical exit exceeds contract bounds"
        );
        let inputs = tx
            .input_points()?
            .iter()
            .map(|op| history.cell(op))
            .collect::<Result<Vec<_>>>()?;
        let prev: Vec<_> = inputs
            .iter()
            .filter(|c| c.output.type_script.as_ref() == Some(&rollup))
            .collect();
        ensure!(prev.len() == 1, "History has no unique rollup predecessor");
        let point = prev[0].out_point.clone();
        let outputs: Vec<_> = tx
            .outputs
            .iter()
            .enumerate()
            .filter(|(_, o)| o.type_script.as_ref() == Some(&rollup))
            .collect();
        ensure!(
            outputs.len() == 1 && quantity(&cursor.index)? == outputs[0].0 as u64,
            "History output is not the unique rollup"
        );
        chain.push((tx, inputs, point.clone(), cursor));
        cursor = point;
    }
    let bootstrap = history.cell(&anchor)?;
    ensure!(
        bootstrap.output.type_script.as_ref() == Some(&rollup),
        "Bootstrap rollup type differs from profile"
    );
    ensure!(
        bootstrap.output.lock.packed()?.as_slice() == p.bootstrap_lock.as_slice()
            && quantity(&bootstrap.output.capacity)? == p.bootstrap_capacity
            && bytes(&bootstrap.output_data)? == p.bootstrap_state.as_slice(),
        "Bootstrap anchor bytes differ from profile"
    );
    let mut journal = Vec::new();
    for (tx, inputs, previous, next) in chain.into_iter().rev() {
        let transition = resolved(&tx, &inputs)?;
        let a = action(&tx, p)?;
        let prior = inputs
            .iter()
            .find(|c| c.out_point == previous)
            .context("Missing predecessor")?;
        ensure!(
            GlobalState::parse(&bytes(&prior.output_data)?)?.account_root == hx(state.root()),
            "Recovery history is discontinuous"
        );
        // Match every witness value to locally reconstructed state before applying the verified delta.
        for (k, v) in &a.kv {
            ensure!(
                state.read(*k)? == *v,
                "History witness disagrees with reconstructed storage"
            );
        }
        let outcome = contract::verify(p, cfg, &transition, &a)
            .map_err(|e| anyhow::anyhow!("Invalid historical exit: {e:?}"))?;
        let before = hx(state.root());
        state.apply(&a, &outcome)?;
        journal.push(Replay {
            transaction_hash: tx.hash()?,
            input_rollup: previous,
            output_rollup: next,
            before_root: before,
            after_root: hx(state.root()),
        });
    }
    ensure!(
        GlobalState::parse(&bytes(&current.output_data)?)?.account_root == hx(state.root()),
        "Reconstructed L2 root does not equal live rollup root"
    );
    history.recheck()?;
    ensure!(
        same_cell(&live(rpc, &current.out_point)?, current),
        "Rollup changed during reconstruction"
    );
    Ok((state, journal))
}
pub fn verify_dependencies(
    rpc: &impl Chain,
    d: &Deployment,
    p: &contract::Profile,
) -> Result<Vec<Cell>> {
    let mut cells = Vec::new();
    let mut seen = BTreeSet::new();
    for dep in &d.cell_deps {
        ensure!(
            dep.dep_type == "code",
            "Use explicit code dependencies, expand dep groups in the deployment manifest"
        );
        ensure!(seen.insert(dep.out_point.clone()), "Duplicate dependency");
        let c = live(rpc, &dep.out_point)?;
        ensure!(
            hx(hash(&bytes(&c.output_data)?)) == dep.data_hash,
            "Dependency bytes differ from manifest"
        );
        cells.push(c);
    }
    ensure!(
        cells.iter().any(|c| c.output_data == d.profile),
        "Missing profile dependency"
    );
    ensure!(
        cells
            .iter()
            .any(|c| hash(&bytes(&c.output_data).unwrap_or_default()) == p.config_hash),
        "Missing RollupConfig dependency"
    );
    let validators: Vec<_> = cells
        .iter()
        .filter(|c| {
            c.output
                .type_script
                .as_ref()
                .and_then(|s| s.hash().ok())
                .as_deref()
                == Some(&hx(p.rollup.code_hash().as_slice()))
        })
        .collect();
    ensure!(
        validators.len() == 1
            && hx(hash(&bytes(&validators[0].output_data)?)) == d.validator_data_hash,
        "Upgraded validator dependency missing or differs from deployment"
    );
    if cells.is_empty() {
        bail!("Missing dependencies")
    };
    Ok(cells)
}
