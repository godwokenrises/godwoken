//! Persisted synthetic L1 history for cold-process replay against an immutable real DB.
//! Host consensus only: never signs, executes CKB-VM, accesses RPC or broadcasts.
use super::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Archive {
    format: String,
    manifest: String,
    deployment: Deployment,
    bundle: Bundle,
    transactions: BTreeMap<String, Transaction>,
    spent: BTreeSet<OutPoint>,
}
impl Chain for Archive {
    fn call(&self, method: &str, params: Value) -> Result<Value> {
        let header = json!({"hash":hx([0x77;32]),"number":"0x1"});
        Ok(match method {
            "get_tip_header" => json!({"hash":hx([0x78;32]),"number":"0x100000"}),
            "get_header" | "get_header_by_number" => header,
            "get_transaction" => {
                let h = params[0].as_str().context("Transaction hash")?;
                let tx = self
                    .transactions
                    .get(h)
                    .context("Missing archived history")?;
                json!({"transaction":tx,"tx_status":{"status":"committed","block_hash":hx([0x77;32])}})
            }
            "get_live_cell" => {
                let point: OutPoint = serde_json::from_value(params[0].clone())?;
                if self.spent.contains(&point) {
                    json!({"status":"dead"})
                } else {
                    let cell = self
                        .transactions
                        .get(&point.tx_hash)
                        .context("Missing archived cell")?
                        .cell(&point)?;
                    json!({"status":"live","cell":{"output":cell.output,"data":{"content":cell.output_data}}})
                }
            }
            _ => anyhow::bail!("Rehearsal forbids RPC method {method}"),
        })
    }
}
impl Archive {
    fn insert(&mut self, tx: Transaction) -> Result<()> {
        for p in tx.input_points()? {
            self.spent.insert(p);
        }
        ensure!(
            self.transactions.insert(tx.hash()?, tx).is_none(),
            "Duplicate archived transaction"
        );
        Ok(())
    }
    fn seed(&mut self, output: CellOutput, data: String) -> Result<Cell> {
        let tx = Transaction {
            version: "0x0".into(),
            cell_deps: vec![],
            header_deps: vec![],
            inputs: vec![
                json!({"since":"0x0","previous_output":{"tx_hash":hx([0xfe;32]),"index":format!("0x{:x}",self.transactions.len())}}),
            ],
            outputs: vec![output],
            outputs_data: vec![data],
            witnesses: vec!["0x".into()],
        };
        let cell = tx.cell(&OutPoint {
            tx_hash: tx.hash()?,
            index: "0x0".into(),
        })?;
        self.insert(tx)?;
        Ok(cell)
    }
    pub fn initialize(plan: &plan::Plan, d: Deployment, bundle: Bundle) -> Result<Self> {
        let mut p = Profile::load(&d)?;
        p.bind(plan)?;
        let mut a = Self {
            format: "gw-synthetic-cold-replay-v1-NOT-FOR-DEPLOYMENT".into(),
            manifest: plan.manifest.clone(),
            deployment: d,
            bundle,
            transactions: BTreeMap::new(),
            spent: BTreeSet::new(),
        };
        a.bundle.rollup_cell = a.seed(
            a.bundle.rollup_cell.output.clone(),
            a.bundle.rollup_cell.output_data.clone(),
        )?;
        let anchor = withdraw::outpoint(&a.bundle.rollup_cell.out_point)?;
        match &mut p {
            Profile::V1(p) => p.base.bootstrap_outpoint = anchor,
            Profile::V0(p) => {
                p.settlement.base.bootstrap_outpoint = anchor;
                for aux in &mut p.auxiliary {
                    let cell = a.seed(withdraw::cell_output(&aux.before)?, hx(&aux.before.data))?;
                    aux.outpoint = withdraw::outpoint(&cell.out_point)?;
                }
            }
        }
        let raw = match &p {
            Profile::V1(p) => s::codec::encode_profile(p),
            Profile::V0(p) => c::v0::encode_profile(p),
        };
        a.deployment.profile = hx(&raw);
        a.deployment.profile_hash = hx(hash(&raw));
        let custody = a
            .bundle
            .cells
            .get("custodian")
            .context("Custody absent")?
            .clone();
        let mut cells = Vec::new();
        for c in custody {
            cells.push(a.seed(c.output, c.output_data)?);
        }
        a.bundle.cells.insert("custodian".into(), cells);
        let sponsor = a.seed(
            CellOutput {
                capacity: format!("0x{:x}", 100000u64 * 100000000),
                lock: Script::from_packed(p.base.governance.clone())?,
                type_script: None,
            },
            "0x".into(),
        )?;
        a.deployment.sponsor_inputs = vec![sponsor.out_point];
        Ok(a)
    }
    pub fn step(
        &mut self,
        db: &dyn NativeDb,
        plan: &plan::Plan,
        addresses: Option<&[String]>,
        payments: Option<&[PaymentSelection]>,
    ) -> Result<(Skeleton, Value)> {
        ensure!(
            self.format == "gw-synthetic-cold-replay-v1-NOT-FOR-DEPLOYMENT"
                && self.manifest == plan.manifest,
            "Wrong rehearsal archive/release"
        );
        let p = Profile::load(&self.deployment)?;
        p.bind(plan)?;
        let original = db.root();
        let (native, ledger, journal) = replay::reconstruct_for(
            db,
            self,
            &p,
            plan,
            &bytes(&self.bundle.config_data)?,
            &self.bundle.rollup_cell,
            self.deployment.confirmations,
        )?;
        let before = remaining_coverage(&p, &self.bundle, &ledger, &native)?;
        let sponsors = self
            .deployment
            .sponsor_inputs
            .iter()
            .map(|o| chain::live(self, o))
            .collect::<Result<_>>()?;
        let history_len = journal.len();
        let sk = if let Some((op, records)) = ledger.next_with_profile_hash(p.hash(), 4)? {
            ensure!(
                addresses.is_none() && payments.is_none(),
                "Initialize roots before selecting applicants"
            );
            admin(
                db,
                &native,
                &ledger,
                &self.deployment,
                &self.bundle,
                op,
                0,
                records,
                sponsors,
                vec![],
                json!(journal),
            )?
        } else {
            let (records, requests) = if let Some(selected) = payments {
                select_payments(&ledger, &p, p.protocol(), selected)?
            } else {
                select_version(
                    &ledger,
                    &p,
                    addresses.context("Supply applicants or partial payments")?,
                    p.protocol(),
                )?
            };
            build::construct(
                db,
                &ledger,
                records,
                0,
                &native,
                &self.deployment,
                &self.bundle,
                &requests,
                sponsors,
                vec![],
                json!(journal),
            )?
        };
        withdraw::verify_size(&sk.transaction)?;
        let action = replay::action_for(&sk.transaction, &p)?;
        // Constructor uses the same native proof and cell verifier as production.
        p.verify(
            &bytes(&self.bundle.config_data)?,
            &chain::resolved(&sk.transaction, &sk.input_cells)?,
            &action,
        )?;
        drop(native);
        drop(ledger);
        self.insert(sk.transaction.clone())?;
        let consumed = sk.transaction.input_points()?;
        let mut custody = self.bundle.cells["custodian"]
            .iter()
            .filter(|c| !consumed.contains(&c.out_point))
            .cloned()
            .collect::<Vec<_>>();
        let (_, cfg) = adapter::validate(&self.bundle, p.protocol())?;
        self.deployment.sponsor_inputs.clear();
        for (i, o) in sk.transaction.outputs.iter().enumerate() {
            let cell = sk.transaction.cell(&OutPoint {
                tx_hash: sk.transaction.hash()?,
                index: format!("0x{i:x}"),
            })?;
            if o.type_script
                .as_ref()
                .is_some_and(|s| s.packed().ok().as_ref() == Some(&p.base.rollup))
            {
                self.bundle.rollup_cell = cell;
            } else if o.lock.code_hash == cfg.hashes["custodian"]
                && o.lock.hash_type == "type"
                && bytes(&o.lock.args)?.starts_with(&c::script_hash(&p.base.rollup))
            {
                custody.push(cell);
            } else if o.type_script.is_none() && o.lock.packed()? == p.base.governance {
                self.deployment.sponsor_inputs.push(cell.out_point);
            }
        }
        self.bundle.cells.insert("custodian".into(), custody);
        // Reconstruct afresh even within this invocation. Persist no SMT/paid-state cache.
        let (native, ledger, journal) = replay::reconstruct_for(
            db,
            self,
            &p,
            plan,
            &bytes(&self.bundle.config_data)?,
            &self.bundle.rollup_cell,
            self.deployment.confirmations,
        )?;
        let after = remaining_coverage(&p, &self.bundle, &ledger, &native)?;
        ensure!(
            db.root() == original && hx(native.root()) == sk.after_root,
            "Replay or original DB root changed"
        );
        let claims=ledger.records[s::CLAIM_PLAN].values().map(|raw|{
            let cl:Claim=decode(raw).map_err(|e|anyhow::anyhow!("{e:?}"))?;
            let paid=ledger.paid(p.deployment,&cl)?;
            Ok(json!({"key":hx(claim_key(p.deployment,&cl)),"address":hx(cl.address),"asset":ledger.source(cl.source)?.asset,"cap":cl.cap.to_string(),"paid":paid.to_string(),"remaining":(cl.cap-paid).to_string()}))
        }).collect::<Result<Vec<_>>>()?;
        let result = json!({"status":"unsigned_host_consensus_cold_replay_pass","protocol":p.protocol(),"original_db_root":original,"db_unchanged":true,"history_before":history_len,"history_after":journal.len(),"operation":format!("{:?}",action.op),"before_root":sk.before_root,"after_root":sk.after_root,"before_coverage":before,"after_coverage":after,"claims":claims,"transaction_bytes":sk.transaction.packed()?.as_slice().len(),"broadcast":false,"vm_executed":false});
        Ok((sk, result))
    }
}
