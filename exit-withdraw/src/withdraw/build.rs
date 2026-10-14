use super::{state::RecoveryState, *};
use crate::{db::Database, model::Bundle};
use anyhow::{ensure, Context, Result};
use gw_common::state::*;
use gw_smt::smt::Blake2bHasher;
use gw_types::{registry_address::RegistryAddress, U256};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged, deny_unknown_fields)]
pub enum Request {
    Address(String),
    Amounts {
        eth_address: String,
        #[serde(deserialize_with = "asset_amounts")]
        amounts: BTreeMap<u32, String>,
    },
}
fn asset_amounts<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> std::result::Result<BTreeMap<u32, String>, D::Error> {
    let values = BTreeMap::<String, String>::deserialize(d)?;
    let mut result = BTreeMap::new();
    for (id, value) in values {
        let n = id.parse::<u32>().map_err(serde::de::Error::custom)?;
        if id != n.to_string() {
            return Err(serde::de::Error::custom(
                "Asset IDs must be canonical decimal integers",
            ));
        }
        result.insert(n, value);
    }
    Ok(result)
}
impl Request {
    pub(crate) fn address(&self) -> Result<[u8; 20]> {
        eth_address(match self {
            Self::Address(s) => s,
            Self::Amounts { eth_address, .. } => eth_address,
        })
    }
}
#[derive(Debug, Serialize, Deserialize)]
pub struct Skeleton {
    pub schema_version: u32,
    pub status: String,
    pub transaction: Transaction,
    pub transaction_hash: String,
    pub input_cells: Vec<Cell>,
    pub dependency_cells: Vec<Cell>,
    pub before_root: String,
    pub after_root: String,
    pub profile_hash: String,
    pub fee_shannons: u64,
    pub allocations: serde_json::Value,
    pub signing_groups: serde_json::Value,
    pub journal: serde_json::Value,
}
pub(crate) fn add(map: &mut BTreeMap<contract::Hash, U256>, key: contract::Hash, n: U256) -> Result<()> {
    let v = map.entry(key).or_default();
    *v = v.checked_add(n).context("Amount overflow")?;
    Ok(())
}
pub(crate) fn token(c: &contract::Cell, cfg: &packed::RollupConfig) -> Result<Option<(contract::Hash, U256)>> {
    if let Some(t) = &c.type_ {
        ensure!(
            t.code_hash().as_slice() == cfg.l1_sudt_script_type_hash().as_slice()
                && u8::from(t.hash_type()) == 1
                && t.args().raw_data().len() == 32
                && c.data.len() == 16,
            "Unsupported custodian token"
        );
        Ok(Some((
            contract::script_hash(t),
            U256::from_little_endian(&c.data),
        )))
    } else {
        ensure!(c.data.is_empty(), "Untyped custodian carries data");
        Ok(None)
    }
}
pub(crate) fn change_cells(
    selected: &BTreeMap<contract::Hash, U256>,
    needed: &BTreeMap<contract::Hash, U256>,
    scripts: &BTreeMap<contract::Hash, packed::Script>,
    p: &contract::Profile,
) -> Result<Vec<contract::Cell>> {
    let mut out = Vec::new();
    for (h, n) in selected {
        if *h == [0; 32] {
            continue;
        }
        let remaining = n
            .checked_sub(*needed.get(h).unwrap_or(&U256::zero()))
            .context("Insufficient selected token")?;
        let mut remainder = remaining;
        let mut preserve_zero_type = remainder.is_zero() && !needed.contains_key(h);
        while !remainder.is_zero() || preserve_zero_type {
            preserve_zero_type = false;
            let quantity = remainder.min(U256::from(u128::MAX));
            let mut cell = contract::Cell {
                capacity: 0,
                lock: p.change.clone(),
                type_: Some(
                    scripts
                        .get(h)
                        .context("Missing change type preimage")?
                        .clone(),
                ),
                data: quantity.low_u128().to_le_bytes().to_vec(),
            };
            cell.capacity = cell
                .minimum_capacity()
                .map_err(|e| anyhow::anyhow!("Capacity: {e:?}"))?;
            out.push(cell);
            remainder -= quantity;
            ensure!(
                out.len() <= contract::MAX_OUTPUTS,
                "Too many change cells; split request"
            );
        }
    }
    if out.is_empty() {
        let mut c = contract::Cell {
            capacity: 0,
            lock: p.change.clone(),
            type_: None,
            data: vec![],
        };
        c.capacity = c
            .minimum_capacity()
            .map_err(|e| anyhow::anyhow!("Capacity: {e:?}"))?;
        out.push(c);
    }
    Ok(out)
}
pub fn construct(
    db: &Database,
    state: &RecoveryState<'_>,
    d: &Deployment,
    b: &Bundle,
    requests: &[Request],
    sponsors: Vec<Cell>,
    dependency_cells: Vec<Cell>,
    journal: serde_json::Value,
) -> Result<Skeleton> {
    let p = d.profile()?;
    let (_, cfg) = b.validate()?;
    ensure!(
        p.rollup.as_slice() == rollup_script().packed()?.as_slice(),
        "Profile is not mainnet v1"
    );
    ensure!(
        state.root()
            == hash32(&GlobalState::parse(&bytes(&b.rollup_cell.output_data)?)?.account_root)?,
        "Live root differs from recovery storage"
    );
    ensure!(
        !requests.is_empty() && requests.len() <= contract::MAX_RECIPIENTS,
        "Use 1..8 recipients per batch"
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
    let mut keys = BTreeSet::new();
    let mut asset_ids = BTreeSet::from([1]);
    let mut payouts = Vec::new();
    let mut manifest = Vec::new();
    let mut type_scripts = BTreeMap::new();
    for cell in &b.cells["custodian"] {
        if let Some(t) = &cell.output.type_script {
            type_scripts.insert(hash32(&t.hash()?)?, t.packed()?);
        }
    }
    for (recipient_index, (address, request)) in requests.iter().enumerate() {
        let info = db.lookup_eth(address)?;
        ensure!(
            info["status"] == "registered",
            "Unregistered ETH address {}",
            hx(address)
        );
        let id = u32::try_from(info["account_id"].as_u64().context("Missing account ID")?)?;
        let script: Script = serde_json::from_value(info["script"].clone())?;
        let script = script.packed()?;
        let sh = contract::script_hash(&script);
        let reg = RegistryAddress::new(2, address.to_vec());
        keys.extend([
            build_account_field_key(id, GW_ACCOUNT_SCRIPT_HASH_TYPE),
            build_account_key(2, &build_registry_address_to_script_hash_key(&reg)),
            build_script_hash_to_account_id_key(&sh),
            build_account_key(2, &build_script_hash_to_registry_address_key(&sh)),
        ]);
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
            let balance = contract::amount(state.read(contract::balance_key(*asset_id, *address))?);
            let wanted = match request {
                Request::Address(_) => balance,
                Request::Amounts { amounts, .. } => amounts
                    .get(asset_id)
                    .map(|s| U256::from_dec_str(s))
                    .transpose()
                    .map_err(|e| anyhow::anyhow!("Invalid decimal amount: {e:?}"))?
                    .unwrap_or_default(),
            };
            ensure!(
                wanted <= balance,
                "Requested amount exceeds L2 balance for {} asset {}",
                hx(address),
                asset_id
            );
            if !wanted.is_zero() {
                chosen.insert(*asset_id, wanted);
            }
        }
        let native = chosen.remove(&1).unwrap_or_default();
        let shannons = native / U256::from(contract::SCALE);
        ensure!(
            shannons <= U256::from(u64::MAX),
            "Native allocation too large; split request"
        );
        let mut capacity = shannons.low_u64();
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
                    lock: contract::eth_lock(*address),
                    type_: Some(typ.clone()),
                    data: amount.low_u128().to_le_bytes().to_vec(),
                };
                c.capacity = c
                    .minimum_capacity()
                    .map_err(|e| anyhow::anyhow!("Capacity: {e:?}"))?;
                capacity=capacity.checked_sub(c.capacity).context("Recipient CKB cannot fund token cell capacity; request smaller token batch and explicit CKB amount")?;
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
            let c = contract::Cell {
                capacity,
                lock: contract::eth_lock(*address),
                type_: None,
                data: vec![],
            };
            ensure!(
                capacity
                    >= c.minimum_capacity()
                        .map_err(|e| anyhow::anyhow!("Capacity: {e:?}"))?,
                "No withdrawable assets or native amount below occupied capacity"
            );
            a.payouts.push(contract::Payout {
                output: (1 + payouts.len()) as u32,
                recipient: recipient_index as u32,
                asset: 1,
            });
            payouts.push(c);
        }
        manifest.push(json!({"eth_address":hx(address),"account_id":id,"allocated_capacity_shannons":shannons.to_string(),"native_residue_l2_raw":(contract::amount(state.read(contract::balance_key(1,*address))?)-shannons*U256::from(contract::SCALE)).to_string()}));
    }
    a.payouts.sort_by_key(|p| (p.recipient, p.asset, p.output));
    for id in asset_ids {
        let (_, script) = db.account(id)?;
        a.assets.push(contract::Asset {
            id,
            script: script.packed()?,
        });
        keys.insert(build_account_field_key(id, GW_ACCOUNT_SCRIPT_HASH_TYPE));
    }
    let mut required = BTreeMap::new();
    let mut debits = BTreeMap::<(u32, u32), U256>::new();
    for payment in &a.payouts {
        let c = &payouts[payment.output as usize - 1];
        add(&mut required, [0; 32], c.capacity.into())?;
        *debits.entry((payment.recipient, 1)).or_default() +=
            U256::from(c.capacity) * U256::from(contract::SCALE);
        if let Some((h, n)) = token(c, &cfg.packed)? {
            add(&mut required, h, n)?;
            let v = debits
                .entry((payment.recipient, payment.asset))
                .or_default();
            *v = v.checked_add(n).context("Debit overflow")?;
        }
    }
    let mut candidates = Vec::new();
    let global = GlobalState::parse(&bytes(&b.rollup_cell.output_data)?)?;
    for c in &b.cells["custodian"] {
        let args = bytes(&c.output.lock.args)?;
        let lock = packed::CustodianLockArgs::from_slice(
            args.get(32..).context("Invalid custodian args")?,
        )?;
        if crate::audit::finalized(lock.deposit_finalized_timepoint().unpack(), &global, &cfg) {
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
        change = change_cells(&totals, &required, &type_scripts, &p)?;
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
    let total_sponsor = sponsors.iter().try_fold(0u64, |n, c| {
        n.checked_add(quantity(&c.output.capacity)?)
            .context("Sponsor overflow")
    })?;
    let sponsor = contract::Cell {
        capacity: total_sponsor
            .checked_sub(d.fee_shannons)
            .context("Insufficient sponsor fees")?,
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
    let mut inputs = vec![b.rollup_cell.clone()];
    inputs.extend(selected);
    inputs.extend(sponsors);
    let mut rollout = contract_cell(&b.rollup_cell)?;
    rollout.lock = p.governance.clone();
    let mut outputs = vec![rollout];
    outputs.extend(payouts);
    outputs.extend(change);
    outputs.push(sponsor);
    let mut sums = BTreeMap::<u32, U256>::new();
    let mut writes = BTreeMap::new();
    for ((r, id), n) in &debits {
        let key = contract::balance_key(*id, a.recipients[*r as usize].address);
        keys.insert(key);
        writes.insert(
            key,
            contract::value(
                contract::amount(state.read(key)?)
                    .checked_sub(*n)
                    .context("Balance underflow")?,
            ),
        );
        let v = sums.entry(*id).or_default();
        *v = v.checked_add(*n).context("Supply sum overflow")?;
    }
    for (id, n) in sums {
        let key = contract::supply_key(id);
        keys.insert(key);
        writes.insert(
            key,
            contract::value(
                contract::amount(state.read(key)?)
                    .checked_sub(n)
                    .context("Supply underflow")?,
            ),
        );
    }
    a.kv = keys
        .iter()
        .map(|k| Ok((*k, state.read(*k)?)))
        .collect::<Result<_>>()?;
    let proof_keys: Vec<_> = keys.iter().map(|k| (*k).into()).collect();
    a.proof = state
        .tree
        .merkle_proof(proof_keys.clone())?
        .compile(proof_keys)?;
    let root: contract::Hash = a
        .proof
        .compute_root::<Blake2bHasher>(
            a.kv.iter()
                .map(|(k, v)| ((*k).into(), (*writes.get(k).unwrap_or(v)).into()))
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
    let outcome = contract::verify(&p, &cfg.packed, &transition, &a)
        .map_err(|e| anyhow::anyhow!("Constructed transaction rejected: {e:?}"))?;
    ensure!(
        outcome.root == root && outcome.fee == d.fee_shannons,
        "Internal construction mismatch"
    );
    let payload = contract::codec::encode_action(&a);
    let mut witnesses = vec![packed::WitnessArgs::default(); inputs.len()];
    witnesses[0] = packed::WitnessArgs::new_builder()
        .output_type(Some(gw_types::bytes::Bytes::from(payload)).pack())
        .build();
    let mut groups = BTreeMap::<String, Vec<usize>>::new();
    for (i, c) in inputs.iter().enumerate() {
        if c.output.lock.packed()?.as_slice() == p.governance.as_slice() || i == 0 {
            groups.entry(c.output.lock.hash()?).or_default().push(i);
        }
    }
    let mut signing = Vec::new();
    for (lock_hash, indices) in groups {
        let first = indices[0];
        let gov = inputs[first].output.lock.packed()?.as_slice() == p.governance.as_slice();
        let placeholder = if gov {
            d.multisig_placeholder(&p)?
        } else {
            bytes(
                d.bootstrap_lock_placeholder
                    .as_deref()
                    .context("Bootstrap lock placeholder required")?,
            )?
        };
        ensure!(!placeholder.is_empty(), "Empty signature placeholder");
        witnesses[first] = witnesses[first]
            .clone()
            .as_builder()
            .lock(Some(gw_types::bytes::Bytes::from(placeholder)).pack())
            .build();
        signing.push(json!({"lock_hash":lock_hash,"input_indices":indices,"first_witness_index":first,"kind":if gov{"legacy_multisig"}else{"bootstrap_lock"},"preserve_input_type_and_output_type":true}));
    }
    let transaction = Transaction {
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
    Ok(Skeleton {
        schema_version: 1,
        status: "unsigned_requires_full_signed_vm_validation".into(),
        transaction_hash: transaction.hash()?,
        transaction,
        input_cells: inputs,
        dependency_cells,
        before_root: hx(state.root()),
        after_root: hx(root),
        profile_hash: d.profile_hash.clone(),
        fee_shannons: d.fee_shannons,
        allocations: json!({"recipients":manifest,"payouts":a.payouts.iter().map(|p|json!({"output_index":p.output,"recipient_index":p.recipient,"asset_id":p.asset})).collect::<Vec<_>>()}),
        signing_groups: json!(signing),
        journal,
    })
}
