//! Offline host-consensus rehearsal over the real read-only DB. Never signs or broadcasts.
use super::*;
use crate::report;
use std::{fs, path::Path};
pub fn run(
    db: &dyn NativeDb,
    plan: &plan::Plan,
    d: &Deployment,
    bundle: &Bundle,
    addresses: &[String],
    batch: usize,
    out: &Path,
) -> Result<serde_json::Value> {
    ensure!(!out.exists(), "Output directory already exists");
    let p = Profile::load(d)?;
    ensure!(
        db.protocol() == p.protocol(),
        "DB/profile protocol mismatch"
    );
    p.bind(plan)?;
    let (_, cfg) = adapter::validate(bundle, p.protocol())?;
    ensure!(
        db.root() == GlobalState::parse(&bytes(&bundle.rollup_cell.output_data)?)?.account_root,
        "Snapshot root mismatch"
    );
    let before_coverage = coverage_version(&p, bundle, None, p.protocol())?;
    let mut native = db.overlay()?;
    let mut ledger = state::Ledger::new(plan)?;
    let mut b = bundle.clone();
    let mut sponsor = Cell {
        out_point: OutPoint {
            tx_hash: hx([0xf1; 32]),
            index: "0x0".into(),
        },
        output: CellOutput {
            capacity: format!("0x{:x}", 100_000u64 * 100_000_000),
            lock: Script::from_packed(p.base.governance.clone())?,
            type_script: None,
        },
        output_data: "0x".into(),
        block_number: String::new(),
        tx_index: String::new(),
    };
    let mut operations = BTreeMap::<String, u64>::new();
    let mut max_bytes = 0;
    let mut steps = 0;
    let apply = |sk: &Skeleton,
                 native: &mut RecoveryState<'_>,
                 ledger: &mut state::Ledger,
                 b: &mut Bundle,
                 sponsor: &mut Cell|
     -> Result<()> {
        withdraw::verify_size(&sk.transaction)?;
        let a = replay::action_for(&sk.transaction, &p)?;
        let e = p.evaluate(&a)?;
        state::apply_native(native, &e, hash32(&sk.after_root)?)?;
        ledger.apply(e)?;
        let consumed = sk.transaction.input_points()?;
        let mut cells: Vec<_> = b.cells["custodian"]
            .iter()
            .filter(|c| !consumed.contains(&c.out_point))
            .cloned()
            .collect();
        let txhash = sk.transaction.hash()?;
        for (i, c) in sk.transaction.outputs.iter().enumerate() {
            let point = OutPoint {
                tx_hash: txhash.clone(),
                index: format!("0x{i:x}"),
            };
            if i == 0 {
                b.rollup_cell = sk.transaction.cell(&point)?;
            } else if c.lock.code_hash == cfg.hashes["custodian"]
                && c.lock.hash_type == "type"
                && bytes(&c.lock.args)?.starts_with(&c::script_hash(&p.base.rollup))
            {
                cells.push(sk.transaction.cell(&point)?);
            } else if c.lock.packed()?.as_slice() == p.base.governance.as_slice() {
                *sponsor = sk.transaction.cell(&point)?;
            }
        }
        b.cells.insert("custodian".into(), cells);
        Ok(())
    };
    while let Some((op, records)) = ledger.next_with_profile_hash(p.hash(), batch)? {
        ensure!(steps < 100_000, "Simulation step limit");
        let sk = admin(
            db,
            &native,
            &ledger,
            d,
            &b,
            op,
            0,
            records,
            vec![sponsor.clone()],
            vec![],
            json!([]),
        )?;
        max_bytes = max_bytes.max(sk.transaction.packed()?.as_slice().len());
        apply(&sk, &mut native, &mut ledger, &mut b, &mut sponsor)?;
        *operations.entry(format!("{op:?}")).or_default() += 1;
        steps += 1;
        if steps % 100 == 0 {
            eprintln!("Verified {steps} administrative transitions ({op:?})");
        }
    }
    fs::create_dir_all(out)?;
    let mut payouts = vec![];
    let mut blocked = vec![];
    let mut seen = BTreeSet::new();
    for address in addresses {
        ensure!(seen.insert(eth_address(address)?), "Duplicate applicant");
        let (records, requests) =
            select_version(&ledger, &p, std::slice::from_ref(address), p.protocol())?;
        let amounts = match &requests[0] {
            withdraw::build::Request::Amounts { amounts, .. } => amounts,
            _ => unreachable!(),
        };
        let ckb = amounts
            .get(&1)
            .map(|v| U256::from_dec_str(v))
            .transpose()
            .map_err(|e| anyhow::anyhow!("{e:?}"))?
            .unwrap_or_default()
            / U256::from(p.protocol().native().scale());
        let mut required = 0u64;
        for (id, amount) in amounts.iter().filter(|(id, _)| **id != 1) {
            let h = p
                .base
                .catalogue
                .iter()
                .find(|(i, _)| i == id)
                .context("Unknown asset")?
                .1;
            let typ = b.cells["custodian"]
                .iter()
                .filter_map(|c| c.output.type_script.as_ref())
                .find(|s| s.hash().ok() == Some(hx(h)))
                .context("Missing custodian token type")?
                .packed()?;
            let n = U256::from_dec_str(amount).map_err(|e| anyhow::anyhow!("{e:?}"))?;
            let count = (n / U256::from(u128::MAX)
                + if n % U256::from(u128::MAX) == U256::zero() {
                    U256::zero()
                } else {
                    U256::one()
                })
            .low_u64();
            let min = c::Cell {
                capacity: 0,
                lock: c::eth_lock(eth_address(address)?),
                type_: Some(typ),
                data: vec![0; 16],
            }
            .minimum_capacity()
            .map_err(|e| anyhow::anyhow!("{e:?}"))?;
            required = required
                .checked_add(min.checked_mul(count).context("Capacity overflow")?)
                .context("Capacity overflow")?;
        }
        if required == 0 {
            required = c::Cell {
                capacity: 0,
                lock: c::eth_lock(eth_address(address)?),
                type_: None,
                data: vec![],
            }
            .minimum_capacity()
            .map_err(|e| anyhow::anyhow!("{e:?}"))?;
        }
        if ckb < U256::from(required) {
            blocked.push(json!({"address":address,"reason":"insufficient_owned_ckb_for_occupied_capacity","required_capacity_shannons":required.to_string(),"available_capacity_shannons":ckb.to_string(),"top_up_shannons":(U256::from(required)-ckb).to_string(),"remaining_assets":amounts}));
            continue;
        }
        let sk = build::construct(
            db,
            &ledger,
            records,
            0,
            &native,
            d,
            &b,
            &requests,
            vec![sponsor.clone()],
            vec![],
            json!([]),
        )?;
        let payrows:Vec<_>=sk.allocations["payouts"].as_array().context("Missing payouts")?.iter().map(|v|{
            let i=v["output_index"].as_u64().expect("builder index") as usize;
            json!({"asset_id":v["asset_id"],"output_index":i,"output":sk.transaction.outputs[i],"data":sk.transaction.outputs_data[i]})
        }).collect();
        max_bytes = max_bytes.max(sk.transaction.packed()?.as_slice().len());
        let filename = format!("applicant-{}.json", payouts.len() + 1);
        report::write_new(&out.join(&filename), &serde_json::to_vec_pretty(&sk)?)?;
        apply(&sk, &mut native, &mut ledger, &mut b, &mut sponsor)?;
        if payouts.len() % 100 == 0 {
            eprintln!(
                "Paid {} applicants; {} capacity blocked",
                payouts.len(),
                blocked.len()
            );
        }
        payouts.push(json!({"address":address,"skeleton":filename,"before_root":sk.before_root,"after_root":sk.after_root,"cells":payrows}));
    }
    let after_coverage = remaining_coverage(&p, &b, &ledger, &native)?;
    let mut asset_rows = vec![];
    for l in &p.limits {
        let a: AssetBudget = ledger.get(s::ASSETS, asset_key(p.deployment, l.asset))?;
        let supply = match p.protocol().native().supply_key(l.asset) {
            Some(k) => c::amount(native.read(k)?),
            None => l.ceiling - a.paid,
        };
        ensure!(
            l.ceiling.checked_sub(a.paid).context("Ceiling underflow")? <= supply,
            "Unpaid rights exceed remaining L2 supply"
        );
        asset_rows.push(json!({"asset_id":l.asset,"paid_raw":a.paid.to_string(),"allocated_raw":l.ceiling.to_string(),"remaining_native_supply_raw":if p.protocol()==Protocol::V1 {Some(supply.to_string())}else{None},"unpaid_ceiling_raw":(l.ceiling-a.paid).to_string()}));
    }
    let summary = json!({"status":"host_consensus_simulation_pass","simulation_only":true,"full_vm_executed":false,"signed":false,"broadcast":false,"db_root_unchanged":db.root()==plan.snapshot_root,"snapshot_root":plan.snapshot_root,"simulated_root":hx(native.root()),"plan_manifest":plan.manifest,"batch_size":batch,"administrative_transactions":steps,"operations":operations,"max_unsigned_transaction_bytes":max_bytes,"protocol":p.protocol(),"payouts":payouts,"capacity_blocked":blocked,"asset_counters":asset_rows,"before_coverage":before_coverage,"after_coverage":after_coverage,"warning":"Artificial fee sponsor; unsigned host consensus rehearsal only. This does not validate live code deployment, signatures, cycles or input liveness."});
    report::write_new(
        &out.join("summary.json"),
        &serde_json::to_vec_pretty(&summary)?,
    )?;
    Ok(summary)
}

/// Publish compact CSV views of an existing host rehearsal; this does not rerun consensus.
pub fn summarize(
    plan: &plan::Plan,
    report_path: &Path,
    output: &Path,
) -> Result<serde_json::Value> {
    use sha2::{Digest, Sha256};
    let checked = plan.verify()?;
    let raw = fs::read(report_path)?;
    let report: serde_json::Value = serde_json::from_slice(&raw)?;
    ensure!(
        report["status"] == "host_consensus_simulation_pass"
            && report["protocol"] == json!(plan.protocol)
            && report["plan_manifest"] == plan.manifest
            && report["snapshot_root"] == plan.snapshot_root
            && report["db_root_unchanged"] == true,
        "Report is not this plan's passing rehearsal"
    );
    ensure!(!output.exists(), "Output directory exists");
    let integer = |v: &serde_json::Value| -> Result<U256> {
        U256::from_dec_str(v.as_str().context("Missing raw amount")?)
            .map_err(|e| anyhow::anyhow!("{e:?}"))
    };
    let mut baseline = BTreeMap::<u32, U256>::new();
    for raw in &plan.sources {
        let s: Source = plan::parse(raw)?;
        let n = baseline.entry(s.asset).or_default();
        *n = n.checked_add(s.baseline).context("Source overflow")?;
    }
    let mut expected = BTreeMap::<(String, u32), U256>::new();
    let maps = plan.records()?;
    let sources = &maps[0];
    for raw in &plan.claims {
        let claim: Claim = plan::parse(raw)?;
        let source: Source =
            decode(&sources[&claim.source]).map_err(|e| anyhow::anyhow!("{e:?}"))?;
        let n = expected
            .entry((hx(claim.address), source.asset))
            .or_default();
        *n = n.checked_add(claim.cap).context("Entitlement overflow")?;
    }
    let mut observed = BTreeMap::<(String, u32), U256>::new();
    let mut paid = BTreeMap::<u32, U256>::new();
    let mut accounts = csv::Writer::from_writer(Vec::new());
    accounts.write_record([
        "eth_owner",
        "asset_id",
        "paid_l1_raw",
        "before_root",
        "after_root",
    ])?;
    for row in report["payouts"].as_array().context("Payouts")? {
        let address = row["address"].as_str().context("Owner")?;
        eth_address(address)?;
        let mut amounts = BTreeMap::<u32, U256>::new();
        for cell in row["cells"].as_array().context("Cells")? {
            let capacity: U256 =
                quantity(cell["output"]["capacity"].as_str().context("Capacity")?)?.into();
            let n = amounts.entry(1).or_default();
            *n = n.checked_add(capacity).context("Capacity overflow")?;
            if !cell["output"]["type"].is_null() {
                let id = u32::try_from(cell["asset_id"].as_u64().context("Asset")?)?;
                ensure!(id != 1, "Typed CKB payout");
                let data = bytes(cell["data"].as_str().context("Token data")?)?;
                ensure!(data.len() == 16, "Invalid sUDT amount");
                let n = amounts.entry(id).or_default();
                *n = n
                    .checked_add(U256::from_little_endian(&data))
                    .context("Token overflow")?;
            }
        }
        for (id, n) in amounts {
            accounts.write_record([
                address.to_owned(),
                id.to_string(),
                n.to_string(),
                row["before_root"].as_str().context("Old root")?.into(),
                row["after_root"].as_str().context("New root")?.into(),
            ])?;
            let n = if id == 1 {
                n.checked_mul(plan.protocol.native().scale().into())
                    .context("Scale overflow")?
            } else {
                n
            };
            let own = observed.entry((address.to_owned(), id)).or_default();
            *own = own.checked_add(n).context("Owner total overflow")?;
            let total = paid.entry(id).or_default();
            *total = total.checked_add(n).context("Paid overflow")?;
        }
    }
    let mut assets = csv::Writer::from_writer(Vec::new());
    assets.write_record([
        "asset_id",
        "type_hash",
        "l2_sources_before_raw",
        "paid_raw",
        "l2_sources_remaining_raw",
        "unpaid_ceiling_raw",
        "custodian_remaining_raw",
        "covered",
    ])?;
    let mut reconciliation = Vec::new();
    for row in report["asset_counters"]
        .as_array()
        .context("Asset counters")?
    {
        let id = u32::try_from(row["asset_id"].as_u64().context("Asset ID")?)?;
        let spent = paid.remove(&id).unwrap_or_default();
        ensure!(
            integer(&row["paid_raw"])? == spent,
            "Payout cells differ from paid counter"
        );
        let old = baseline.get(&id).copied().unwrap_or_default();
        let left = old.checked_sub(spent).context("L2 source underflow")?;
        let coverage = report["after_coverage"]["assets"]
            .as_array()
            .context("Coverage")?
            .iter()
            .find(|r| r["asset_id"] == id)
            .context("Missing custody asset")?;
        let unpaid = integer(&coverage["unpaid_ceiling_raw"])?;
        let custody = integer(&coverage["available_raw"])?;
        ensure!(
            coverage["covered"] == true && unpaid <= custody && unpaid <= left,
            "Uncovered remaining rights"
        );
        if let Some(native) = report["after_coverage"]["remaining_native_sources"].as_array() {
            let actual = native
                .iter()
                .find(|r| r["asset_id"] == id)
                .map(|r| integer(&r["raw"]))
                .transpose()?
                .unwrap_or_default();
            ensure!(
                actual == left,
                "Replayed native source totals differ from conservation"
            );
        }
        let limit = plan
            .limits
            .iter()
            .map(|r| plan::parse::<AssetLimit>(r))
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .find(|l| l.asset == id)
            .context("Missing asset limit")?;
        assets.write_record([
            id.to_string(),
            hx(limit.type_hash),
            old.to_string(),
            spent.to_string(),
            left.to_string(),
            unpaid.to_string(),
            custody.to_string(),
            "true".into(),
        ])?;
        reconciliation.push(json!({"asset_id":id,"type_hash":hx(limit.type_hash),"l2_sources_remaining_raw":left.to_string(),"paid_raw":spent.to_string(),"custodian_remaining_raw":custody.to_string(),"unpaid_ceiling_raw":unpaid.to_string()}));
    }
    ensure!(paid.is_empty(), "Payout asset missing from counters");
    let mut blocked = csv::Writer::from_writer(Vec::new());
    blocked.write_record([
        "eth_owner",
        "required_shannons",
        "available_shannons",
        "top_up_shannons",
        "remaining_assets_json",
    ])?;
    let mut top_up = U256::zero();
    for row in report["capacity_blocked"]
        .as_array()
        .context("Blocked accounts")?
    {
        let needed = integer(&row["required_capacity_shannons"])?;
        let available = integer(&row["available_capacity_shannons"])?;
        let extra = integer(&row["top_up_shannons"])?;
        ensure!(
            available.checked_add(extra) == Some(needed) && !extra.is_zero(),
            "Bad capacity report"
        );
        top_up = top_up
            .checked_add(extra)
            .context("Capacity total overflow")?;
        for (asset, value) in row["remaining_assets"]
            .as_object()
            .context("Blocked asset amounts")?
        {
            let key = (
                row["address"].as_str().context("Owner")?.to_owned(),
                asset.parse::<u32>()?,
            );
            let n = observed.entry(key).or_default();
            *n = n
                .checked_add(integer(value)?)
                .context("Blocked amount overflow")?;
        }
        blocked.write_record([
            row["address"].as_str().context("Owner")?.to_owned(),
            needed.to_string(),
            available.to_string(),
            extra.to_string(),
            serde_json::to_string(&row["remaining_assets"])?,
        ])?;
    }
    for (key, n) in &observed {
        ensure!(
            expected.get(key).is_some_and(|cap| n <= cap),
            "Reported payout/blocked rights exceed planned account rights"
        );
    }
    let all_accounted = observed == expected;
    if plan.protocol == Protocol::V0
        && observed
            .keys()
            .map(|(a, _)| a)
            .collect::<BTreeSet<_>>()
            .len()
            == expected
                .keys()
                .map(|(a, _)| a)
                .collect::<BTreeSet<_>>()
                .len()
    {
        ensure!(
            all_accounted,
            "V0 all-account report has missing or incorrect assets"
        );
    }
    fs::create_dir_all(output)?;
    for (name, w) in [
        ("account-payouts.csv", accounts),
        ("asset-reconciliation.csv", assets),
        ("capacity-blocked.csv", blocked),
    ] {
        report::write_new(&output.join(name), &w.into_inner()?)?;
    }
    let paid_count = report["payouts"].as_array().unwrap().len();
    let blocked_count = report["capacity_blocked"].as_array().unwrap().len();
    let mut summary = report;
    summary.as_object_mut().unwrap().remove("payouts");
    summary.as_object_mut().unwrap().remove("capacity_blocked");
    summary["all_plan_beneficiaries_and_assets_accounted"] = json!(all_accounted);
    summary["paid_accounts"] = json!(paid_count);
    summary["capacity_blocked_accounts"] = json!(blocked_count);
    summary["top_up_shannons"] = json!(top_up.to_string());
    summary["source_report_sha256"] = json!(hex::encode(Sha256::digest(&raw)));
    summary["plan_validation"] = checked;
    summary["asset_reconciliation"] = json!(reconciliation);
    report::write_new(
        &output.join("validation.json"),
        &serde_json::to_vec_pretty(&summary)?,
    )?;
    Ok(summary)
}
