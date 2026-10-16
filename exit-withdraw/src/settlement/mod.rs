pub mod adapter;
pub mod build;
pub mod cli;
pub mod layout;
pub mod plan;
pub mod rehearsal;
pub mod replay;
pub mod signing;
pub mod simulation;
pub mod state;
pub mod table;
pub mod v0_plan;
use crate::{
    model::*,
    settlement::adapter::{NativeDb, Profile, Protocol},
    withdraw::{
        self,
        build::{Request, Skeleton},
        chain::{self, Chain},
        state::RecoveryState,
        Deployment, Transaction,
    },
};
use anyhow::{ensure, Context, Result};
use exit_withdraw_state_validator::{
    self as c,
    settlement::{self as s, records::*},
};
use gw_smt::smt::Blake2bHasher;
use gw_types::{packed, prelude::*, U256};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
pub fn profile(d: &Deployment) -> Result<s::SettlementProfile> {
    s::codec::decode_profile(&bytes(&d.profile)?, hash32(&d.profile_hash)?)
        .map_err(|e| anyhow::anyhow!("Settlement profile: {e:?}"))
}
pub fn admin(
    db: &dyn NativeDb,
    native: &RecoveryState<'_>,
    ledger: &state::Ledger,
    d: &Deployment,
    bundle: &Bundle,
    op: s::Operation,
    epoch: u64,
    records: Vec<Vec<u8>>,
    sponsors: Vec<Cell>,
    deps: Vec<Cell>,
    journal: serde_json::Value,
) -> Result<Skeleton> {
    let p = Profile::load(d)?;
    ensure!(
        db.protocol() == p.protocol(),
        "DB/profile protocol mismatch"
    );
    adapter::validate(bundle, p.protocol())?;
    ensure!(
        hx(native.root())
            == GlobalState::parse(&bytes(&bundle.rollup_cell.output_data)?)?.account_root,
        "Stale rollup"
    );
    let a = ledger.build_action(
        db,
        native,
        &p,
        hash32(&d.profile_hash)?,
        op,
        epoch,
        records,
        state::empty_action(),
    )?;
    let e = p.evaluate(&a)?;
    let root: c::Hash = a
        .native
        .proof
        .compute_root::<Blake2bHasher>(
            a.native
                .kv
                .iter()
                .map(|(k, v)| ((*k).into(), (*e.native_writes.get(k).unwrap_or(v)).into()))
                .collect(),
        )?
        .into();
    let mut inputs = vec![bundle.rollup_cell.clone()];
    let mut auxiliary_outputs = Vec::new();
    if op == s::Operation::Begin {
        if let Profile::V0(profile) = &p {
            for aux in &profile.auxiliary {
                inputs.push(Cell {
                    out_point: withdraw::from_outpoint(&aux.outpoint),
                    output: withdraw::cell_output(&aux.before)?,
                    output_data: hx(&aux.before.data),
                    block_number: String::new(),
                    tx_index: String::new(),
                });
                auxiliary_outputs.push(aux.after.clone());
            }
        }
    }
    let sponsor_start = inputs.len();
    inputs.extend(sponsors);
    ensure!(
        inputs.len() > sponsor_start,
        "Governance fee sponsor input required"
    );
    let mut rollup = withdraw::contract_cell(&bundle.rollup_cell)?;
    let gs = packed::GlobalState::from_slice(&rollup.data)?;
    rollup.data = gs
        .clone()
        .as_builder()
        .account(gs.account().as_builder().merkle_root(root.pack()).build())
        .build()
        .as_slice()
        .to_vec();
    rollup.lock = p.base.governance.clone();
    let capacity = inputs[sponsor_start..]
        .iter()
        .try_fold(0u64, |v, c| {
            v.checked_add(quantity(&c.output.capacity)?)
                .context("Sponsor overflow")
        })?
        .checked_sub(d.fee_shannons)
        .context("Sponsor cannot cover fee")?;
    let mut out = vec![
        rollup,
        c::Cell {
            capacity,
            lock: p.base.governance.clone(),
            type_: None,
            data: vec![],
        },
    ];
    let sponsor_output = out.pop().context("Missing fee change")?;
    out.extend(auxiliary_outputs);
    out.push(sponsor_output);
    let tx = c::Transaction {
        inputs: inputs
            .iter()
            .map(withdraw::contract_cell)
            .collect::<Result<_>>()?,
        outputs: out,
        outpoints: inputs
            .iter()
            .map(|c| withdraw::outpoint(&c.out_point))
            .collect::<Result<_>>()?,
    };
    build::finish(
        native,
        d,
        &p,
        &bytes(&bundle.config_data)?,
        inputs,
        tx,
        deps,
        journal,
        &a,
        root,
        json!([]),
    )
}
pub fn select(
    ledger: &state::Ledger,
    p: &s::SettlementProfile,
    addresses: &[String],
) -> Result<(Vec<Vec<u8>>, Vec<Request>)> {
    select_version(ledger, p, addresses, Protocol::V1)
}
pub fn select_version(
    ledger: &state::Ledger,
    p: &s::SettlementProfile,
    addresses: &[String],
    protocol: Protocol,
) -> Result<(Vec<Vec<u8>>, Vec<Request>)> {
    ensure!(
        ledger.metadata.is_some(),
        "Refund roots are not initialized; run the Begin step first"
    );
    let mut wanted = BTreeSet::new();
    for a in addresses {
        ensure!(
            wanted.insert(eth_address(a)?),
            "Duplicate applicant address"
        );
    }
    ensure!(
        !wanted.is_empty() && wanted.len() <= c::MAX_RECIPIENTS,
        "Select 1..{} unique applicants",
        c::MAX_RECIPIENTS
    );
    let mut selected = BTreeMap::<[u8; 20], Vec<Payment>>::new();
    for raw in ledger.records[s::CLAIM_PLAN].values() {
        let claim: Claim = decode(raw).map_err(|e| anyhow::anyhow!("{e:?}"))?;
        if wanted.contains(&claim.address) {
            let n = claim
                .cap
                .checked_sub(ledger.paid(p.deployment, &claim)?)
                .context("Corrupt paid amount")?;
            if !n.is_zero() {
                selected
                    .entry(claim.address)
                    .or_default()
                    .push(Payment { claim, amount: n });
            }
        }
    }
    let mut records = Vec::new();
    let mut requests = Vec::new();
    for addr in wanted {
        let claims = selected
            .get_mut(&addr)
            .context("Applicant has no remaining approved rights")?;
        let mut amounts = BTreeMap::<u32, U256>::new();
        for pay in claims.iter() {
            let source = ledger.source(pay.claim.source)?;
            let v = amounts.entry(source.asset).or_default();
            *v = v.checked_add(pay.amount).context("Overflow")?;
        }
        let native = amounts.get(&1).copied().unwrap_or_default();
        let mut dust = native % U256::from(protocol.native().scale());
        amounts.insert(1, native - dust);
        for pay in claims.iter_mut().rev() {
            let source = ledger.source(pay.claim.source)?;
            if source.asset == 1 && !dust.is_zero() {
                let n = pay.amount.min(dust);
                pay.amount -= n;
                dust -= n;
            }
        }
        for pay in claims.iter().filter(|p| !p.amount.is_zero()) {
            records.push(encode(pay));
        }
        requests.push(Request::Amounts {
            eth_address: hx(addr),
            amounts: amounts
                .into_iter()
                .filter(|(_, n)| !n.is_zero())
                .map(|(id, n)| (id, n.to_string()))
                .collect(),
        });
    }
    ensure!(
        records.len() <= s::MAX_RECORDS,
        "Selected claims exceed 32; use a smaller applicant batch"
    );
    Ok((records, requests))
}
pub fn prepare(
    db: &dyn NativeDb,
    rpc: &impl Chain,
    d: &Deployment,
    bundle: &Bundle,
    plan: &plan::Plan,
    addresses: Option<&[String]>,
    batch: usize,
) -> Result<Skeleton> {
    prepare_with_controls(db, rpc, d, bundle, plan, addresses, batch, &[])
}
pub fn prepare_with_controls(
    db: &dyn NativeDb,
    rpc: &impl Chain,
    d: &Deployment,
    bundle: &Bundle,
    plan: &plan::Plan,
    addresses: Option<&[String]>,
    batch: usize,
    controls: &[build::ControlRequest],
) -> Result<Skeleton> {
    prepare_selected(db, rpc, d, bundle, plan, addresses, batch, controls, None)
}
#[derive(Clone, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct PaymentSelection {
    pub claim_key: String,
    pub amount_raw: String,
}
pub fn select_payments(
    ledger: &state::Ledger,
    p: &s::SettlementProfile,
    protocol: Protocol,
    selected: &[PaymentSelection],
) -> Result<(Vec<Vec<u8>>, Vec<Request>)> {
    ensure!(
        ledger.metadata.is_some() && !selected.is_empty() && selected.len() <= s::MAX_RECORDS,
        "Select 1..{} claims from an active release",
        s::MAX_RECORDS
    );
    let mut seen = BTreeSet::new();
    let mut sums = BTreeMap::<[u8; 20], BTreeMap<u32, U256>>::new();
    let mut records = Vec::new();
    for item in selected {
        let key = hash32(&item.claim_key)?;
        ensure!(seen.insert(key), "Duplicate payment claim");
        let claim: Claim = ledger.get(s::CLAIM_PLAN, key)?;
        ensure!(
            claim_key(p.deployment, &claim) == key && claim.epoch == 0,
            "Wrong claim epoch or key"
        );
        ensure!(
            !item.amount_raw.is_empty()
                && item.amount_raw.bytes().all(|b| b.is_ascii_digit())
                && !item.amount_raw.starts_with('0'),
            "Use a positive canonical raw amount"
        );
        let amount = U256::from_dec_str(&item.amount_raw).map_err(|e| anyhow::anyhow!("{e:?}"))?;
        ensure!(
            amount
                <= claim
                    .cap
                    .checked_sub(ledger.paid(p.deployment, &claim)?)
                    .context("Corrupt paid state")?,
            "Payment exceeds remaining entitlement"
        );
        let src = ledger.source(claim.source)?;
        let q = sums
            .entry(claim.address)
            .or_default()
            .entry(src.asset)
            .or_default();
        *q = q.checked_add(amount).context("Overflow")?;
        records.push(encode(&Payment { claim, amount }));
    }
    let mut requests = vec![];
    for (address, amounts) in sums {
        ensure!(
            amounts
                .get(&1)
                .is_some_and(|n| *n % U256::from(protocol.native().scale()) == U256::zero()),
            "Each payment needs CKB in whole L1 shannons"
        );
        requests.push(Request::Amounts {
            eth_address: hx(address),
            amounts: amounts
                .into_iter()
                .map(|(id, n)| (id, n.to_string()))
                .collect(),
        });
    }
    Ok((records, requests))
}
pub fn prepare_selected(
    db: &dyn NativeDb,
    rpc: &impl Chain,
    d: &Deployment,
    bundle: &Bundle,
    plan: &plan::Plan,
    addresses: Option<&[String]>,
    batch: usize,
    controls: &[build::ControlRequest],
    payments: Option<&[PaymentSelection]>,
) -> Result<Skeleton> {
    prepare_selected_funding(
        db, rpc, d, bundle, plan, addresses, batch, controls, payments, None,
    )
}
pub fn prepare_selected_funding(
    db: &dyn NativeDb,
    rpc: &impl Chain,
    d: &Deployment,
    bundle: &Bundle,
    plan: &plan::Plan,
    addresses: Option<&[String]>,
    batch: usize,
    controls: &[build::ControlRequest],
    payments: Option<&[PaymentSelection]>,
    funding: Option<&build::CapacityFunding>,
) -> Result<Skeleton> {
    let p = Profile::load(d)?;
    ensure!(
        db.protocol() == p.protocol(),
        "DB/profile protocol mismatch"
    );
    adapter::validate(bundle, p.protocol())?;
    let deps = chain::verify_dependencies(rpc, d, &p.base)?;
    let (native, ledger, journal) = replay::reconstruct_for(
        db,
        rpc,
        &p,
        plan,
        &bytes(&bundle.config_data)?,
        &bundle.rollup_cell,
        d.confirmations,
    )?;
    remaining_coverage(&p, bundle, &ledger, &native)?;
    let sponsors = d
        .sponsor_inputs
        .iter()
        .map(|p| chain::live(rpc, p))
        .collect::<Result<Vec<_>>>()?;
    let out = if let Some((op, records)) = ledger.next_with_profile_hash(p.hash(), batch)? {
        ensure!(
            addresses.is_none() && payments.is_none() && controls.is_empty() && funding.is_none(),
            "Refund roots are not initialized; omit selections and complete the Begin step"
        );
        admin(
            db,
            &native,
            &ledger,
            d,
            bundle,
            op,
            0,
            records,
            sponsors,
            deps,
            serde_json::to_value(journal)?,
        )?
    } else {
        let (records, requests) = if let Some(selected) = payments {
            select_payments(&ledger, &p, p.protocol(), selected)?
        } else {
            select_version(
                &ledger,
                &p,
                addresses.context("Epoch active; supply --addresses, --controls or --payments")?,
                p.protocol(),
            )?
        };
        let controls = controls
            .iter()
            .map(|r| {
                Ok(build::ControlSpend {
                    account_id: r.account_id,
                    destination: crate::claim::parse_address(&r.destination)?,
                    authorization_cells: r
                        .authorization_inputs
                        .iter()
                        .map(|p| chain::live(rpc, p))
                        .collect::<Result<Vec<_>>>()?,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        build::construct_with_controls_funding(
            db,
            &ledger,
            records,
            0,
            &native,
            d,
            bundle,
            &requests,
            sponsors,
            deps,
            serde_json::to_value(journal)?,
            &controls,
            funding,
        )?
    };
    withdraw::verify_script_dependencies(
        &out.input_cells,
        &out.transaction.outputs,
        &out.dependency_cells,
    )?;
    withdraw::verify_size(&out.transaction)?;
    for cell in out.input_cells.iter().chain(&out.dependency_cells) {
        ensure!(
            chain::same_cell(&chain::live(rpc, &cell.out_point)?, cell),
            "Input changed during build"
        );
    }
    Ok(out)
}
pub fn verify_signed(
    db: &dyn NativeDb,
    rpc: &impl Chain,
    d: &Deployment,
    bundle: &Bundle,
    plan: &plan::Plan,
    tx: &Transaction,
    max_cycles: u64,
) -> Result<serde_json::Value> {
    ensure!(
        max_cycles > 0 && max_cycles <= 70_000_000,
        "Cycle cap must be 1..70,000,000"
    );
    let p = Profile::load(d)?;
    ensure!(
        db.protocol() == p.protocol(),
        "DB/profile protocol mismatch"
    );
    adapter::validate(bundle, p.protocol())?;
    let deps = chain::verify_dependencies(rpc, d, &p.base)?;
    withdraw::verify_size(tx)?;
    let (native, ledger, _) = replay::reconstruct_for(
        db,
        rpc,
        &p,
        plan,
        &bytes(&bundle.config_data)?,
        &bundle.rollup_cell,
        d.confirmations,
    )?;
    let backing = remaining_coverage(&p, bundle, &ledger, &native)?;
    ensure!(
        tx.cell_deps
            == d.cell_deps
                .iter()
                .map(|x| json!({"out_point":x.out_point,"dep_type":x.dep_type}))
                .collect::<Vec<_>>(),
        "Dependencies differ from manifest"
    );
    let inputs = tx
        .input_points()?
        .iter()
        .map(|p| chain::live(rpc, p))
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        inputs
            .iter()
            .any(|c| chain::same_cell(c, &bundle.rollup_cell)),
        "Stale rollup input"
    );
    withdraw::verify_script_dependencies(&inputs, &tx.outputs, &deps)?;
    let a = replay::action_for(tx, &p)?;
    for (k, v) in &a.native.kv {
        ensure!(
            native.read(*k)? == *v,
            "Witness state differs from reconstructed state"
        );
    }
    let out = p.verify(
        &bytes(&bundle.config_data)?,
        &chain::resolved(tx, &inputs)?,
        &a,
    )?;
    ensure!(
        out.fee == d.fee_shannons,
        "Fee differs from approved manifest"
    );
    let result = rpc.call("estimate_cycles", json!([tx]))?;
    let cycles = quantity(result["cycles"].as_str().context("Missing cycles")?)?;
    ensure!(
        cycles <= max_cycles,
        "Full signed transaction exceeds cycle budget; reduce batch"
    );
    for c in inputs.iter().chain(&deps) {
        ensure!(
            chain::same_cell(&chain::live(rpc, &c.out_point)?, c),
            "Input changed during validation"
        );
    }
    let sponsor_spend = out
        .fee
        .checked_add(out.top_up_shannons)
        .context("Sponsor spend overflow")?;
    Ok(
        json!({"status":"verified_not_broadcast","transaction_hash":tx.hash()?,"before_root":hx(native.root()),"after_root":hx(out.root),"cycles":cycles,"operation":format!("{:?}",a.op),"remaining_coverage":backing,"capacity_funding":{"account_lock_hash":hx(c::script_hash(&p.base.governance)),"actual_top_up_ckb":ckb_amount(out.top_up_shannons),"actual_top_up_shannons":out.top_up_shannons.to_string(),"fee_ckb":ckb_amount(out.fee),"fee_shannons":out.fee.to_string(),"actual_total_spend_ckb":ckb_amount(sponsor_spend),"actual_total_spend_shannons":sponsor_spend.to_string()}}),
    )
}

/// Check the entire unpaid ceiling against the observed pool, not just this batch's inputs.
/// Ceilings deliberately do not authorize reopening any retained disposition.
pub fn coverage(
    p: &s::SettlementProfile,
    b: &Bundle,
    ledger: Option<&state::Ledger>,
) -> Result<serde_json::Value> {
    coverage_version(p, b, ledger, Protocol::V1)
}
pub fn coverage_version(
    p: &s::SettlementProfile,
    b: &Bundle,
    ledger: Option<&state::Ledger>,
    protocol: Protocol,
) -> Result<serde_json::Value> {
    let (_, cfg) = adapter::validate(b, protocol)?;
    let mut custody = BTreeMap::<c::Hash, U256>::new();
    let mut types = BTreeMap::new();
    let mut capacity = U256::zero();
    for cell in &b.cells["custodian"] {
        // Capture can include pending deposits; only finalized custodian cells fund this release.
        let lock = cell.output.lock.packed()?;
        ensure!(
            u8::from(lock.hash_type()) == 1
                && lock.code_hash().as_slice()
                    == cfg.packed.custodian_script_type_hash().as_slice(),
            "Wrong custodian code"
        );
        let args = lock.args().raw_data();
        ensure!(
            args.len() >= 32 && args[..32] == c::script_hash(&p.base.rollup),
            "Wrong custodian rollup"
        );
        if !adapter::finalized(cell, b, &cfg, protocol)? {
            continue;
        }
        capacity = capacity
            .checked_add(
                U256::from(quantity(&cell.output.capacity)?)
                    * U256::from(protocol.native().scale()),
            )
            .context("Capacity overflow")?;
        if let Some((h, n)) = withdraw::build::token(&withdraw::contract_cell(cell)?, &cfg.packed)?
        {
            let v = custody.entry(h).or_default();
            *v = v.checked_add(n).context("Custody overflow")?;
            types.insert(
                h,
                cell.output
                    .type_script
                    .as_ref()
                    .context("Missing type")?
                    .packed()?,
            );
        }
    }
    let mut remaining_ckb = U256::zero();
    let mut rows = vec![];
    for limit in &p.limits {
        let paid = match ledger
            .and_then(|l| l.records[s::ASSETS].get(&asset_key(p.deployment, limit.asset)))
        {
            Some(b) => {
                decode::<AssetBudget>(b)
                    .map_err(|e| anyhow::anyhow!("{e:?}"))?
                    .paid
            }
            None => U256::zero(),
        };
        let remaining = limit
            .ceiling
            .checked_sub(paid)
            .context("Paid exceeds ceiling")?;
        let available = if limit.asset == 1 {
            capacity
        } else {
            custody.get(&limit.type_hash).copied().unwrap_or_default()
        };
        ensure!(
            remaining <= available,
            "Remaining ceiling for asset {} exceeds finalized live custodian",
            limit.asset
        );
        if limit.asset == 1 {
            remaining_ckb = remaining;
        } else if let Some(n) = custody.get_mut(&limit.type_hash) {
            *n -= remaining;
        }
        rows.push(json!({"asset_id":limit.asset,"unpaid_ceiling_raw":remaining.to_string(),"available_raw":available.to_string(),"covered":true}));
    }
    let mut reserve_capacity = 0u64;
    for (h, _) in custody.iter().filter(|(_, n)| !n.is_zero()) {
        let cell = c::Cell {
            capacity: 0,
            lock: p.base.change.clone(),
            type_: Some(types.get(h).context("Missing residual type")?.clone()),
            data: vec![0; 16],
        };
        reserve_capacity = reserve_capacity
            .checked_add(
                cell.minimum_capacity()
                    .map_err(|e| anyhow::anyhow!("{e:?}"))?,
            )
            .context("Capacity overflow")?;
    }
    if reserve_capacity == 0 && capacity > remaining_ckb {
        reserve_capacity = c::Cell {
            capacity: 0,
            lock: p.base.change.clone(),
            type_: None,
            data: vec![],
        }
        .minimum_capacity()
        .map_err(|e| anyhow::anyhow!("{e:?}"))?;
    }
    ensure!(
        capacity - remaining_ckb
            >= U256::from(reserve_capacity) * U256::from(protocol.native().scale()),
        "Unpaid CKB ceiling would consume retained custodians' minimum capacity"
    );
    Ok(
        json!({"assets":rows,"minimum_retained_custodian_capacity_shannons":reserve_capacity,"all_unpaid_ceilings_covered":true}),
    )
}

fn remaining_coverage(
    p: &Profile,
    bundle: &Bundle,
    ledger: &state::Ledger,
    native: &RecoveryState<'_>,
) -> Result<serde_json::Value> {
    let mut report = coverage_version(p, bundle, Some(ledger), p.protocol())?;
    if p.protocol() == Protocol::V0 {
        let mut totals = BTreeMap::<u32, U256>::new();
        for raw in ledger.records[s::SOURCE_PLAN].values() {
            let src: Source = decode(raw).map_err(|e| anyhow::anyhow!("{e:?}"))?;
            let paid = ledger.records[s::SOURCES]
                .get(&source_key(p.deployment, &src))
                .map(|v| decode::<SourceBudget>(v).map(|b| b.paid))
                .transpose()
                .map_err(|e| anyhow::anyhow!("{e:?}"))?
                .unwrap_or_default();
            let remaining = p
                .protocol()
                .native()
                .amount(native.read(p.protocol().native().balance_key(src.asset, src.address))?)
                .map_err(|e| anyhow::anyhow!("{e:?}"))?;
            ensure!(
                remaining == src.baseline.checked_sub(paid).context("Source underflow")?,
                "v0 remaining source differs from native state"
            );
            let sum = totals.entry(src.asset).or_default();
            *sum = sum
                .checked_add(remaining)
                .context("Remaining L2 source total overflow")?;
        }
        report["remaining_native_sources"] = json!(totals
            .iter()
            .map(|(id, n)| json!({"asset_id":id,"raw":n.to_string()}))
            .collect::<Vec<_>>());
        return Ok(report);
    }
    for l in &p.limits {
        let paid = ledger.records[s::ASSETS]
            .get(&asset_key(p.deployment, l.asset))
            .map(|v| decode::<AssetBudget>(v).map(|v| v.paid))
            .transpose()
            .map_err(|e| anyhow::anyhow!("{e:?}"))?
            .unwrap_or_default();
        ensure!(
            l.ceiling
                .checked_sub(paid)
                .context("Corrupt asset counter")?
                <= c::amount(native.read(c::supply_key(l.asset))?),
            "Unpaid ceiling exceeds native L2 supply for asset {}",
            l.asset
        );
    }
    Ok(report)
}
