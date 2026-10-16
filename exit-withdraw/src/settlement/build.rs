//! Settlement cell construction reuses the existing custody/capacity primitives.
use crate::{
    model::*,
    settlement::adapter::{self, NativeDb, Profile},
    withdraw::{
        build::{add, change_cells, token, Request, Skeleton},
        state::RecoveryState,
        *,
    },
};
use anyhow::{ensure, Context, Result};
use exit_withdraw_state_validator::{self as contract, settlement as settle};
use gw_smt::smt::Blake2bHasher;
use gw_types::{packed, prelude::*, U256};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
pub fn construct(
    db: &dyn NativeDb,
    ledger: &super::state::Ledger,
    records: Vec<Vec<u8>>,
    epoch: u64,
    state: &RecoveryState<'_>,
    d: &Deployment,
    b: &Bundle,
    requests: &[Request],
    sponsors: Vec<Cell>,
    dependency_cells: Vec<Cell>,
    journal: serde_json::Value,
) -> Result<Skeleton> {
    construct_with_controls(
        db,
        ledger,
        records,
        epoch,
        state,
        d,
        b,
        requests,
        sponsors,
        dependency_cells,
        journal,
        &[],
    )
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlRequest {
    pub account_id: u32,
    pub destination: String,
    pub authorization_inputs: Vec<OutPoint>,
}
pub struct ControlSpend {
    pub account_id: u32,
    pub destination: Script,
    pub authorization_cells: Vec<Cell>,
}
/// Explicit governance account and maximum total spend, including the transaction fee.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapacityFunding {
    pub account_lock_hash: String,
    pub max_spend_ckb: String,
}
impl CapacityFunding {
    pub fn max_spend_shannons(&self) -> Result<u64> {
        let mut parts = self.max_spend_ckb.split('.');
        let whole = parts.next().unwrap_or_default();
        let fractional = parts.next();
        ensure!(parts.next().is_none(), "Invalid max_spend_ckb");
        ensure!(
            !whole.is_empty() && whole.bytes().all(|b| b.is_ascii_digit()),
            "Invalid max_spend_ckb"
        );
        let whole: u64 = whole.parse()?;
        let mut amount = whole
            .checked_mul(100_000_000)
            .context("CKB limit overflow")?;
        if let Some(fractional) = fractional {
            ensure!(
                !fractional.is_empty()
                    && fractional.len() <= 8
                    && fractional.bytes().all(|b| b.is_ascii_digit()),
                "max_spend_ckb must have at most 8 decimal places"
            );
            let padding = 8 - fractional.len();
            let fraction: u64 = fractional.parse()?;
            amount = amount
                .checked_add(fraction * 10u64.pow(padding as u32))
                .context("CKB limit overflow")?;
        }
        Ok(amount)
    }
}
pub fn construct_with_controls(
    db: &dyn NativeDb,
    ledger: &super::state::Ledger,
    records: Vec<Vec<u8>>,
    epoch: u64,
    state: &RecoveryState<'_>,
    d: &Deployment,
    b: &Bundle,
    requests: &[Request],
    sponsors: Vec<Cell>,
    dependency_cells: Vec<Cell>,
    journal: serde_json::Value,
    controls: &[ControlSpend],
) -> Result<Skeleton> {
    construct_with_controls_funding(
        db,
        ledger,
        records,
        epoch,
        state,
        d,
        b,
        requests,
        sponsors,
        dependency_cells,
        journal,
        controls,
        None,
    )
}
pub fn construct_with_controls_funding(
    db: &dyn NativeDb,
    ledger: &super::state::Ledger,
    records: Vec<Vec<u8>>,
    epoch: u64,
    state: &RecoveryState<'_>,
    d: &Deployment,
    b: &Bundle,
    requests: &[Request],
    sponsors: Vec<Cell>,
    dependency_cells: Vec<Cell>,
    journal: serde_json::Value,
    controls: &[ControlSpend],
    funding: Option<&CapacityFunding>,
) -> Result<Skeleton> {
    let sp = Profile::load(d)?;
    ensure!(
        db.protocol() == sp.protocol(),
        "DB/profile protocol mismatch"
    );
    let p = &sp.base;
    let funding_limit = funding
        .map(|policy| -> Result<u64> {
            ensure!(
                hash32(&policy.account_lock_hash)? == contract::script_hash(&p.governance),
                "Capacity funder must be the governance account in this Pay version"
            );
            policy.max_spend_shannons()
        })
        .transpose()?;
    let (_, cfg) = adapter::validate(b, sp.protocol())?;
    ensure!(
        hx(contract::script_hash(&p.rollup)) == db.protocol().rollup(),
        "Profile is not the selected mainnet protocol"
    );
    ensure!(
        state.root()
            == hash32(&GlobalState::parse(&bytes(&b.rollup_cell.output_data)?)?.account_root)?,
        "Live root differs from recovery storage"
    );
    ensure!(
        !requests.is_empty() && requests.len() <= contract::MAX_RECIPIENTS,
        "Use 1..{} recipients per batch",
        contract::MAX_RECIPIENTS
    );
    let mut requests = requests
        .iter()
        .map(|r| Ok((r.address()?, r)))
        .collect::<Result<Vec<_>>>()?;
    requests.sort_by_key(|(a, _)| *a);
    ensure!(
        requests.windows(2).all(|w| w[0].0 != w[1].0),
        "Duplicate withdrawal address"
    );
    let mut a = contract::Action {
        recipients: Vec::new(),
        assets: Vec::new(),
        payouts: Vec::new(),
        kv: Vec::new(),
        proof: gw_smt::smt::CompiledMerkleProof(vec![]),
    };
    let mut asset_ids = BTreeSet::from([1]);
    let mut payouts = Vec::new();
    let mut manifest = Vec::new();
    let mut top_up_total = 0u64;
    let mut type_scripts = BTreeMap::new();
    for cell in &b.cells["custodian"] {
        if let Some(t) = &cell.output.type_script {
            type_scripts.insert(hash32(&t.hash()?)?, t.packed()?);
        }
    }
    for (recipient_index, (address, request)) in requests.iter().enumerate() {
        let descriptor = ledger.recipient_scripts.iter().find_map(|(id, s)| {
            s.packed()
                .ok()
                .filter(|s| contract::control::identity(s) == *address)
                .map(|s| (*id, s))
        });
        let (id, script, destination) = if let Some((id, script)) = descriptor {
            let control = controls.iter().find(|c| c.account_id == id).context(
                "Control payout requires destination and controller authorization inputs",
            )?;
            (id, script, control.destination.packed()?)
        } else {
            let info = db.lookup_eth(address)?;
            ensure!(
                info["status"] == "registered",
                "Unregistered ETH address {}",
                hx(address)
            );
            let id = u32::try_from(info["account_id"].as_u64().context("Missing account ID")?)?;
            let script: Script = serde_json::from_value(info["script"].clone())?;
            (id, script.packed()?, contract::eth_lock(*address))
        };
        a.recipients.push(contract::Recipient {
            address: *address,
            id,
            script,
        });
        let mut chosen = BTreeMap::new();
        if let Request::Amounts { amounts, .. } = request {
            ensure!(
                amounts
                    .keys()
                    .all(|id| p.catalogue.iter().any(|(known, _)| known == id)),
                "Request contains unsupported asset ID"
            );
        }
        for (asset_id, _) in &p.catalogue {
            let wanted = match request {
                Request::Address(_) => {
                    anyhow::bail!("Settlement builder needs explicit approved amounts")
                }
                Request::Amounts { amounts, .. } => amounts
                    .get(asset_id)
                    .map(|s| U256::from_dec_str(s))
                    .transpose()
                    .map_err(|e| anyhow::anyhow!("Invalid decimal amount: {e:?}"))?
                    .unwrap_or_default(),
            };
            if !wanted.is_zero() {
                chosen.insert(*asset_id, wanted);
            }
        }
        let native = chosen.remove(&1).unwrap_or_default();
        let shannons = native / U256::from(sp.protocol().native().scale());
        ensure!(
            shannons <= U256::from(u64::MAX),
            "Native allocation too large; split request"
        );
        let mut capacity = shannons.low_u64();
        let mut top_up = 0u64;
        let start = payouts.len();
        for (asset_id, mut n) in chosen {
            let h = p
                .catalogue
                .iter()
                .find(|(id, _)| *id == asset_id)
                .context("Unknown asset")?
                .1;
            let typ = if let Some(t) = type_scripts.get(&h) {
                t.clone()
            } else {
                db.script(&h, true)?
                    .context("Missing L1 asset Script preimage")?
                    .packed()?
            };
            type_scripts.insert(h, typ.clone());
            while !n.is_zero() {
                let amount = n.min(U256::from(u128::MAX));
                let mut c = contract::Cell {
                    capacity: 0,
                    lock: destination.clone(),
                    type_: Some(typ.clone()),
                    data: amount.low_u128().to_le_bytes().to_vec(),
                };
                c.capacity = c
                    .minimum_capacity()
                    .map_err(|e| anyhow::anyhow!("Capacity: {e:?}"))?;
                if capacity >= c.capacity {
                    capacity -= c.capacity;
                } else {
                    top_up = top_up
                        .checked_add(c.capacity - capacity)
                        .context("Capacity top-up overflow")?;
                    capacity = 0;
                }
                a.payouts.push(contract::Payout {
                    output: (1 + payouts.len()) as u32,
                    recipient: recipient_index as u32,
                    asset: asset_id,
                });
                payouts.push(c);
                asset_ids.insert(asset_id);
                n -= amount;
                ensure!(
                    payouts.len() < contract::MAX_OUTPUTS - 2,
                    "Too many payouts; split request"
                );
            }
        }
        if payouts.len() > start {
            payouts[start].capacity = payouts[start]
                .capacity
                .checked_add(capacity)
                .context("Capacity overflow")?;
        } else {
            let mut c = contract::Cell {
                capacity,
                lock: destination.clone(),
                type_: None,
                data: vec![],
            };
            let minimum = c
                .minimum_capacity()
                .map_err(|e| anyhow::anyhow!("Capacity: {e:?}"))?;
            if capacity < minimum {
                top_up = top_up
                    .checked_add(minimum - capacity)
                    .context("Capacity top-up overflow")?;
                c.capacity = minimum;
            }
            a.payouts.push(contract::Payout {
                output: (1 + payouts.len()) as u32,
                recipient: recipient_index as u32,
                asset: 1,
            });
            payouts.push(c);
        }
        top_up_total = top_up_total
            .checked_add(top_up)
            .context("Capacity top-up overflow")?;
        manifest.push(json!({"eth_address":hx(address),"account_id":id,"allocated_capacity_shannons":shannons.to_string(),"capacity_top_up_ckb":ckb_amount(top_up),"capacity_top_up_shannons":top_up.to_string(),"payout_capacity_shannons":(shannons + U256::from(top_up)).to_string(),"native_residue_l2_raw":(native-shannons*U256::from(sp.protocol().native().scale())).to_string()}));
    }
    if top_up_total != 0 {
        ensure!(
            funding.is_some(),
            "Payout cells need {top_up_total} shannons of external capacity; provide --capacity-funding with the governance account and a maximum total CKB spend"
        );
    }
    let sponsor_spend = d
        .fee_shannons
        .checked_add(top_up_total)
        .context("Sponsor spend overflow")?;
    if let Some(limit) = funding_limit {
        ensure!(
            sponsor_spend <= limit,
            "Governance account would spend {sponsor_spend} shannons including fee and capacity top-up, above configured maximum {limit} shannons"
        );
    }
    a.payouts.sort_by_key(|p| (p.recipient, p.asset, p.output));
    for id in asset_ids {
        let (_, script) = db.account(id)?;
        a.assets.push(contract::Asset {
            id,
            script: script.packed()?,
        });
    }
    let mut required = BTreeMap::new();
    for payment in &a.payouts {
        let c = &payouts[payment.output as usize - 1];
        add(&mut required, [0; 32], c.capacity.into())?;
        if let Some((h, n)) = token(c, &cfg.packed)? {
            add(&mut required, h, n)?;
        }
    }
    let required_ckb = required
        .get_mut(&[0; 32])
        .context("Missing CKB requirement")?;
    *required_ckb = required_ckb
        .checked_sub(U256::from(top_up_total))
        .context("Capacity top-up exceeds payout capacity")?;
    let mut candidates = Vec::new();
    for c in &b.cells["custodian"] {
        if adapter::finalized(c, b, &cfg, sp.protocol())? {
            candidates.push(c.clone());
        }
    }
    candidates.sort_by(|x, y| x.out_point.cmp(&y.out_point));
    let mut selected = Vec::new();
    let mut totals = BTreeMap::new();
    let mut used = BTreeSet::new();
    for (h, needed) in required.iter().filter(|(h, _)| **h != [0; 32]) {
        for (i, c) in candidates.iter().enumerate() {
            if totals.get(h).copied().unwrap_or_default() >= *needed {
                break;
            }
            if used.contains(&i) {
                continue;
            }
            let cc = contract_cell(c)?;
            if let Some((actual, n)) = token(&cc, &cfg.packed)? {
                if actual == *h {
                    used.insert(i);
                    selected.push(c.clone());
                    add(&mut totals, [0; 32], cc.capacity.into())?;
                    add(&mut totals, actual, n)?;
                }
            }
        }
        ensure!(
            totals.get(h).copied().unwrap_or_default() >= *needed,
            "Insufficient finalized custodian token backing"
        );
    }
    let mut change;
    loop {
        let mut change_profile = std::borrow::Cow::Borrowed(p);
        if sp.protocol() == adapter::Protocol::V0 {
            if let Some(first) = selected.first() {
                let lock = first.output.lock.packed()?;
                if lock.as_slice() != p.change.as_slice() {
                    change_profile.to_mut().change = lock;
                }
            }
        }
        change = change_cells(&totals, &required, &type_scripts, &change_profile)?;
        let min = change.iter().try_fold(0u64, |n, c| {
            n.checked_add(c.capacity)
                .context("Change capacity overflow")
        })?;
        let required_capacity = required[&[0; 32]]
            .checked_add(min.into())
            .context("Capacity overflow")?;
        if totals.get(&[0; 32]).copied().unwrap_or_default() >= required_capacity {
            break;
        }
        let (i, c) = candidates
            .iter()
            .enumerate()
            .find(|(i, _)| !used.contains(i))
            .context("Insufficient custodian capacity for payouts and retained change")?;
        used.insert(i);
        selected.push(c.clone());
        let cc = contract_cell(c)?;
        add(&mut totals, [0; 32], cc.capacity.into())?;
        if let Some((h, n)) = token(&cc, &cfg.packed)? {
            add(&mut totals, h, n)?;
        }
        ensure!(
            selected.len() + 1 + sponsors.len() <= contract::MAX_INPUTS,
            "Too many inputs; split request"
        );
    }
    let change_capacity = totals[&[0; 32]] - required[&[0; 32]];
    ensure!(
        change_capacity <= u64::MAX.into(),
        "Change too large; split request"
    );
    let minimum = change.iter().map(|c| c.capacity).sum::<u64>();
    change[0].capacity += change_capacity.low_u64() - minimum;
    ensure!(
        !sponsors.is_empty(),
        "At least one governance sponsor input required"
    );
    for cell in &sponsors {
        ensure!(
            cell.output.lock.packed()?.as_slice() == p.governance.as_slice()
                && cell.output.type_script.is_none()
                && bytes(&cell.output_data)?.is_empty(),
            "Sponsor input must be a plain CKB cell under the selected governance account"
        );
    }
    let total_sponsor = sponsors.iter().try_fold(0u64, |n, c| {
        n.checked_add(quantity(&c.output.capacity)?)
            .context("Sponsor overflow")
    })?;
    let sponsor = contract::Cell {
        capacity: total_sponsor
            .checked_sub(sponsor_spend)
            .context("Insufficient governance CKB for fee and capacity top-up")?,
        lock: p.governance.clone(),
        type_: None,
        data: vec![],
    };
    ensure!(
        sponsor.capacity
            >= sponsor
                .minimum_capacity()
                .map_err(|e| anyhow::anyhow!("Capacity: {e:?}"))?,
        "Sponsor change below occupied capacity"
    );
    let sponsor_change_capacity = sponsor.capacity;
    let mut inputs = vec![b.rollup_cell.clone()];
    inputs.extend(selected);
    inputs.extend(sponsors);
    let mut rollout = contract_cell(&b.rollup_cell)?;
    rollout.lock = p.governance.clone();
    let mut outputs = vec![rollout];
    outputs.extend(payouts);
    outputs.extend(change);
    let sponsor_change_output_index = outputs.len();
    outputs.push(sponsor);
    let mut control_ids = BTreeSet::new();
    let mut control_points = BTreeSet::new();
    for control in controls {
        ensure!(
            control_ids.insert(control.account_id)
                && a.recipients
                    .iter()
                    .any(|r| r.id == control.account_id
                        && contract::control::is_descriptor(&r.script)),
            "Unused/duplicate control request"
        );
        for cell in &control.authorization_cells {
            if control_points.insert(cell.out_point.clone()) {
                inputs.push(cell.clone());
                outputs.push(contract_cell(cell)?);
            }
        }
    }
    let sa = ledger.build_action(
        db,
        state,
        &sp,
        hash32(&d.profile_hash)?,
        settle::Operation::Pay,
        epoch,
        records,
        a,
    )?;
    let evaluation = sp.evaluate(&sa)?;
    let a = &sa.native;
    let future_warnings =
        future_capacity_warnings(db, ledger, &sp, a, &sa.records, &outputs, &type_scripts)?;
    let root: contract::Hash = a
        .proof
        .compute_root::<Blake2bHasher>(
            a.kv.iter()
                .map(|(k, v)| {
                    (
                        (*k).into(),
                        (*evaluation.native_writes.get(k).unwrap_or(v)).into(),
                    )
                })
                .collect(),
        )?
        .into();
    let gs = packed::GlobalState::from_slice(&outputs[0].data)?;
    outputs[0].data = gs
        .clone()
        .as_builder()
        .account(gs.account().as_builder().merkle_root(root.pack()).build())
        .build()
        .as_slice()
        .to_vec();
    let transition = contract::Transaction {
        inputs: inputs.iter().map(contract_cell).collect::<Result<_>>()?,
        outputs,
        outpoints: inputs
            .iter()
            .map(|c| outpoint(&c.out_point))
            .collect::<Result<_>>()?,
    };
    let mut skeleton = finish(
        state,
        d,
        &sp,
        &bytes(&b.config_data)?,
        inputs,
        transition,
        dependency_cells,
        journal,
        &sa,
        root,
        json!(manifest),
    )?;
    skeleton.allocations["capacity_funding"] = json!({
        "account_lock_hash": hx(contract::script_hash(&p.governance)),
        "maximum_spend_ckb": funding.map(|policy| policy.max_spend_ckb.clone()),
        "maximum_spend_shannons": funding_limit.map(|n| n.to_string()),
        "actual_top_up_ckb": ckb_amount(top_up_total),
        "actual_top_up_shannons": top_up_total.to_string(),
        "fee_ckb": ckb_amount(d.fee_shannons),
        "fee_shannons": d.fee_shannons.to_string(),
        "actual_total_spend_ckb": ckb_amount(sponsor_spend),
        "actual_total_spend_shannons": sponsor_spend.to_string(),
        "returned_change_ckb": ckb_amount(sponsor_change_capacity),
        "returned_change_shannons": sponsor_change_capacity.to_string(),
        "sponsor_change_output_index": sponsor_change_output_index,
        "sponsor_inputs": d.sponsor_inputs,
    });
    skeleton.allocations["future_capacity_warnings"] = json!(future_warnings);
    Ok(skeleton)
}

fn future_capacity_warnings(
    db: &dyn NativeDb,
    ledger: &super::state::Ledger,
    sp: &Profile,
    action: &contract::Action,
    selected: &[Vec<u8>],
    outputs: &[contract::Cell],
    type_scripts: &BTreeMap<contract::Hash, packed::Script>,
) -> Result<Vec<serde_json::Value>> {
    use settle::records::{claim_key, decode, Claim, Payment};

    let mut selected_amounts = BTreeMap::new();
    for raw in selected {
        let payment: Payment = decode(raw).map_err(|e| anyhow::anyhow!("{e:?}"))?;
        selected_amounts.insert(claim_key(sp.deployment, &payment.claim), payment.amount);
    }
    let mut warnings = Vec::new();
    'recipient: for (index, recipient) in action.recipients.iter().enumerate() {
        let mut remaining = BTreeMap::<u32, U256>::new();
        for raw in ledger.records[settle::CLAIM_PLAN].values() {
            let claim: Claim = decode(raw).map_err(|e| anyhow::anyhow!("{e:?}"))?;
            if claim.address != recipient.address {
                continue;
            }
            let paid = ledger.paid(sp.deployment, &claim)?;
            let selected = selected_amounts
                .get(&claim_key(sp.deployment, &claim))
                .copied()
                .unwrap_or_default();
            let left = claim
                .cap
                .checked_sub(paid)
                .and_then(|n| n.checked_sub(selected))
                .context("Selected claim exceeds remaining entitlement")?;
            let asset = ledger.source(claim.source)?.asset;
            let entry = remaining.entry(asset).or_default();
            *entry = entry
                .checked_add(left)
                .context("Remaining balance overflow")?;
        }
        let native = remaining.remove(&1).unwrap_or_default();
        let native_shannons = native / U256::from(sp.protocol().native().scale());
        let Some(payout) = action
            .payouts
            .iter()
            .find(|p| p.recipient as usize == index)
        else {
            continue;
        };
        let destination = outputs
            .get(payout.output as usize)
            .context("Missing payout destination")?
            .lock
            .clone();
        let mut required = U256::zero();
        let mut remaining_token_assets = 0;
        for (asset, amount) in remaining {
            if amount.is_zero() {
                continue;
            }
            remaining_token_assets += 1;
            let type_hash = sp
                .base
                .catalogue
                .iter()
                .find(|(id, _)| *id == asset)
                .context("Unknown remaining asset")?
                .1;
            let typ = if let Some(script) = type_scripts.get(&type_hash) {
                script.clone()
            } else if let Some(script) = db.script(&type_hash, true)? {
                script.packed()?
            } else {
                warnings.push(json!({
                    "eth_address": hx(recipient.address),
                    "account_id": recipient.id,
                    "reason": "remaining_token_type_preimage_unavailable_for_capacity_estimate",
                    "asset_id": asset
                }));
                continue 'recipient;
            };
            let minimum = contract::Cell {
                capacity: 0,
                lock: destination.clone(),
                type_: Some(typ),
                data: vec![0; 16],
            }
            .minimum_capacity()
            .map_err(|e| anyhow::anyhow!("Capacity: {e:?}"))?;
            let max_per_cell = U256::from(u128::MAX);
            let count = amount / max_per_cell
                + if (amount % max_per_cell).is_zero() {
                    U256::zero()
                } else {
                    U256::one()
                };
            required = required
                .checked_add(U256::from(minimum) * count)
                .context("Future capacity overflow")?;
        }
        if remaining_token_assets == 0 {
            if native_shannons.is_zero() {
                continue;
            }
            required = U256::from(
                contract::Cell {
                    capacity: 0,
                    lock: destination,
                    type_: None,
                    data: vec![],
                }
                .minimum_capacity()
                .map_err(|e| anyhow::anyhow!("Capacity: {e:?}"))?,
            );
        }
        if native_shannons < required {
            warnings.push(json!({
                "eth_address": hx(recipient.address),
                "account_id": recipient.id,
                "remaining_native_shannons": native_shannons.to_string(),
                "minimum_future_cell_capacity_shannons": required.to_string(),
                "future_top_up_needed_shannons": (required - native_shannons).to_string(),
                "remaining_token_assets": remaining_token_assets,
                "assumption": "All remaining assets paid to the current destination; future capacity funding requires a separate approved transaction"
            }));
        }
    }
    Ok(warnings)
}

pub fn finish(
    state: &RecoveryState<'_>,
    d: &Deployment,
    sp: &Profile,
    raw_cfg: &[u8],
    inputs: Vec<Cell>,
    transition: contract::Transaction,
    dependency_cells: Vec<Cell>,
    journal: serde_json::Value,
    sa: &settle::SettlementAction,
    root: contract::Hash,
    manifest: serde_json::Value,
) -> Result<Skeleton> {
    let a = &sa.native;
    let outcome = sp.verify(raw_cfg, &transition, sa)?;
    ensure!(
        outcome.root == root && outcome.fee == d.fee_shannons,
        "Internal construction mismatch"
    );
    let payload = sp.encode_action(sa);
    let mut witnesses = vec![packed::WitnessArgs::default(); inputs.len()];
    witnesses[0] = packed::WitnessArgs::new_builder()
        .output_type(Some(gw_types::bytes::Bytes::from(payload)).pack())
        .build();
    let mut signing = Vec::new();
    let mut groups = BTreeMap::<String, Vec<usize>>::new();
    for (i, cell) in inputs.iter().enumerate() {
        if super::signing::lock_template(d, sp, &cell.output.lock.packed()?)?.is_some() {
            groups.entry(cell.output.lock.hash()?).or_default().push(i);
        }
    }
    for (lock_hash, indices) in groups {
        let first = indices[0];
        let (kind, placeholder, _, _, _) =
            super::signing::lock_template(d, sp, &inputs[first].output.lock.packed()?)?
                .context("Signing lock")?;
        witnesses[first] = witnesses[first]
            .clone()
            .as_builder()
            .lock(Some(gw_types::bytes::Bytes::from(placeholder)).pack())
            .build();
        signing.push(json!({"lock_hash":lock_hash,"input_indices":indices,"first_witness_index":first,"kind":kind,"preserve_input_type_and_output_type":true}));
    }
    let mut transaction = Transaction {
        version: "0x0".into(),
        cell_deps: d
            .cell_deps
            .iter()
            .map(|x| json!({"out_point":x.out_point,"dep_type":x.dep_type}))
            .collect(),
        header_deps: vec![],
        inputs: inputs
            .iter()
            .map(|c| json!({"since":"0x0","previous_output":c.out_point}))
            .collect(),
        outputs: transition
            .outputs
            .iter()
            .map(cell_output)
            .collect::<Result<_>>()?,
        outputs_data: transition.outputs.iter().map(|c| hx(&c.data)).collect(),
        witnesses: witnesses.iter().map(|w| hx(w.as_slice())).collect(),
    };
    super::replay::frame_transaction(&mut transaction)?;
    Ok(Skeleton {
        schema_version: settle::FORMAT_VERSION,
        status: "unsigned_requires_full_signed_vm_validation".into(),
        transaction_hash: transaction.hash()?,
        transaction,
        input_cells: inputs,
        dependency_cells,
        before_root: hx(state.root()),
        after_root: hx(root),
        profile_hash: d.profile_hash.clone(),
        fee_shannons: d.fee_shannons,
        allocations: json!({"operation":format!("{:?}",sa.op),"epoch":sa.epoch,"records":sa.records.iter().map(hx).collect::<Vec<_>>(),"recipients":manifest,"payouts":a.payouts.iter().map(|p|json!({"output_index":p.output,"recipient_index":p.recipient,"asset_id":p.asset})).collect::<Vec<_>>(),"capacity_funding":{"account_lock_hash":hx(contract::script_hash(&sp.base.governance)),"actual_top_up_shannons":outcome.top_up_shannons.to_string(),"fee_shannons":outcome.fee.to_string(),"actual_total_spend_shannons":outcome.top_up_shannons.checked_add(outcome.fee).context("Sponsor spend overflow")?.to_string()}}),
        signing_groups: json!(signing),
        journal,
    })
}
