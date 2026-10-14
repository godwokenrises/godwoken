//! A standard ETH OmniLock has no protocol tag. Only verified release payouts and
//! unmixed, same-owner descendants are spendable through the selected portal.
use super::*;
use crate::{
    settlement::{adapter::Profile, replay},
    withdraw::chain::{self, Chain, History},
};
use exit_withdraw_state_validator::settlement::Operation;
pub struct Provenance<'a, C> {
    pub history: History<'a, C>,
    profile: Profile,
    deployment: &'a withdraw::Deployment,
    cells: BTreeMap<OutPoint, bool>,
    settlements: BTreeSet<String>,
    visits: usize,
}
impl<'a, C: Chain> Provenance<'a, C> {
    pub fn new(rpc: &'a C, d: &'a withdraw::Deployment, confirmations: u64) -> Result<Self> {
        Ok(Self {
            history: History::new(rpc, confirmations)?,
            profile: Profile::load(d)?,
            deployment: d,
            cells: BTreeMap::new(),
            settlements: BTreeSet::new(),
            visits: 0,
        })
    }
    fn settlement(&mut self, tx_hash: &str) -> Result<()> {
        let rollup = Script::from_packed(self.profile.base.rollup.clone())?;
        let anchor = withdraw::from_outpoint(&self.profile.base.bootstrap_outpoint);
        if self.settlements.contains(tx_hash) {
            return Ok(());
        }
        // The confirming mainnet node has already executed the pinned validator binary.
        // Authenticate this exact payout, its profile/config/code dependencies and its
        // unique mainnet rollup identity. Full DB/journal replay belongs to operator
        // verification; wallet lookups must not replay thousands of registration txs.
        let bootstrap = self.history.cell(&anchor)?;
        ensure!(
            bootstrap.output.type_script.as_ref() == Some(&rollup)
                && bootstrap.output.lock.packed()?.as_slice()
                    == self.profile.base.bootstrap_lock.as_slice()
                && quantity(&bootstrap.output.capacity)? == self.profile.base.bootstrap_capacity
                && bytes(&bootstrap.output_data)? == self.profile.base.bootstrap_state.as_slice(),
            "Published bootstrap anchor mismatch"
        );
        let tx = self.history.transaction(tx_hash)?;
        ensure!(
            tx.cell_deps
                == self
                    .deployment
                    .cell_deps
                    .iter()
                    .map(|d| json!({"out_point":d.out_point,"dep_type":d.dep_type}))
                    .collect::<Vec<_>>(),
            "Distribution dependencies differ from release"
        );
        let mut deps = Vec::new();
        for d in &self.deployment.cell_deps {
            let cell = self.history.cell(&d.out_point)?;
            ensure!(
                hx(hash(&bytes(&cell.output_data)?)) == d.data_hash,
                "Distribution dependency hash mismatch"
            );
            deps.push(cell);
        }
        let validator = deps
            .iter()
            .filter(|cell| {
                cell.output.type_script.as_ref().and_then(|s| s.hash().ok())
                    == Some(hx(self.profile.base.rollup.code_hash().as_slice()))
            })
            .collect::<Vec<_>>();
        ensure!(
            validator.len() == 1
                && hx(hash(&bytes(&validator[0].output_data)?))
                    == self.deployment.validator_data_hash,
            "Distribution validator is not the published binary"
        );
        ensure!(
            deps.iter()
                .any(|c| c.output_data == self.deployment.profile),
            "Missing published profile dependency"
        );
        let cfg = deps
            .iter()
            .find(|cell| {
                hash(&bytes(&cell.output_data).unwrap_or_default()) == self.profile.base.config_hash
            })
            .context("Missing release config")?;
        let inputs = tx
            .input_points()?
            .iter()
            .map(|p| self.history.cell(p))
            .collect::<Result<Vec<_>>>()?;
        let predecessors = inputs
            .iter()
            .filter(|c| c.output.type_script.as_ref() == Some(&rollup))
            .collect::<Vec<_>>();
        ensure!(predecessors.len() == 1, "No unique rollup input");
        let a = replay::action_for(&tx, &self.profile)?;
        self.profile.verify(
            &bytes(&cfg.output_data)?,
            &chain::resolved(&tx, &inputs)?,
            &a,
        )?;
        ensure!(a.op == Operation::Pay, "Origin is not a refund payout");
        self.settlements.insert(tx_hash.to_owned());
        Ok(())
    }
    pub fn owns(&mut self, point: &OutPoint, owner: &Script) -> Result<bool> {
        self.walk(point, owner, 0)
    }
    fn walk(&mut self, point: &OutPoint, owner: &Script, depth: usize) -> Result<bool> {
        // Cache is owner-specific in use; verify the actual output lock before consulting it.
        let cell = self.history.cell(point)?;
        if cell.output.lock != *owner {
            return Ok(false);
        }
        if let Some(v) = self.cells.get(point) {
            return Ok(*v);
        }
        self.visits += 1;
        ensure!(
            depth <= 128 && self.visits <= 4096,
            "Claim ancestry too large; operator indexing required"
        );
        let tx = self.history.transaction(&point.tx_hash)?;
        let rollup = Script::from_packed(self.profile.base.rollup.clone())?;
        let result = if tx
            .outputs
            .iter()
            .any(|c| c.type_script.as_ref() == Some(&rollup))
        {
            self.settlement(&point.tx_hash)?;
            let a = replay::action_for(&tx, &self.profile)?;
            a.op == Operation::Pay
                && a.native
                    .payouts
                    .iter()
                    .any(|p| u64::from(p.output) == quantity(&point.index).unwrap_or(u64::MAX))
        } else {
            let inputs = tx.input_points()?;
            if inputs.is_empty() || inputs.len() > MAX_INPUTS {
                false
            } else {
                let mut all = true;
                for input in inputs {
                    if !self.walk(&input, owner, depth + 1)? {
                        all = false;
                        break;
                    }
                }
                all
            }
        };
        self.cells.insert(point.clone(), result);
        Ok(result)
    }
}
