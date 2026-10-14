//! TYPE_ID-preserving validator upgrade, funded solely by the code authority.
use crate::{
    model::*,
    settlement::{
        adapter::{Profile, Protocol},
        signing::{Signatures, SigningGroup, SigningRequest},
    },
    withdraw::{
        self,
        chain::{self, Chain},
        Dependency, Deployment, Transaction,
    },
};
use anyhow::{ensure, Context, Result};
use exit_withdraw_state_validator as c;
use gw_types::{packed, prelude::*};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeSet;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub protocol: Protocol,
    pub code_cell: OutPoint,
    pub old_data_hash: String,
    pub multisig_config: String,
    pub funding_inputs: Vec<OutPoint>,
    pub dependencies: Vec<Dependency>,
    pub fee_shannons: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Skeleton {
    pub request: Request,
    pub transaction: Transaction,
    pub input_cells: Vec<Cell>,
    pub dependency_cells: Vec<Cell>,
    pub profile_hash: String,
    pub new_data_hash: String,
    pub code_type_hash: String,
}
fn config(s: &str) -> Result<Vec<u8>> {
    let b = bytes(s)?;
    ensure!(
        b.len() >= 4
            && b[0] == 0
            && b[2] > 0
            && b[1] <= b[2]
            && b[2] <= b[3]
            && b.len() == 4 + 20 * b[3] as usize,
        "Invalid multisig config"
    );
    ensure!(
        b[4..].chunks_exact(20).collect::<BTreeSet<_>>().len() == b[3] as usize,
        "Duplicate multisig member"
    );
    Ok(b)
}
pub fn build(r: &Request, d: &Deployment, binary: &[u8], rpc: &impl Chain) -> Result<Skeleton> {
    let p = Profile::load(d)?;
    ensure!(p.protocol() == r.protocol, "Upgrade protocol mismatch");
    ensure!(
        binary.starts_with(b"\x7fELF") && hx(hash(binary)) == d.validator_data_hash,
        "Binary differs from reviewed deployment hash"
    );
    ensure!(r.fee_shannons > 0, "Upgrade fee must be positive");
    let cfg = config(&r.multisig_config)?;
    let code = chain::live(rpc, &r.code_cell)?;
    ensure!(
        hx(hash(&bytes(&code.output_data)?)) == r.old_data_hash,
        "Stale code hash"
    );
    let ty = code
        .output
        .type_script
        .as_ref()
        .context("Code needs TYPE_ID")?;
    ensure!(
        ty.code_hash == hx([&[0u8; 25][..], b"TYPE_ID"].concat())
            && ty.hash_type == "type"
            && bytes(&ty.args)?.len() == 32,
        "Code cell must use built-in TYPE_ID"
    );
    ensure!(
        ty.hash()? == hx(p.base.rollup.code_hash().as_slice()),
        "Code TYPE_ID does not match selected rollup validator"
    );
    ensure!(
        code.output.lock.code_hash == hx(c::MULTISIG)
            && code.output.lock.hash_type == "type"
            && code.output.lock.args == hx(&hash(&cfg)[..20]),
        "Upgrade multisig preimage differs from code authority"
    );
    let mut inputs = vec![code.clone()];
    let mut seen = BTreeSet::from([code.out_point.clone()]);
    for point in &r.funding_inputs {
        ensure!(seen.insert(point.clone()), "Duplicate upgrade input");
        let cell = chain::live(rpc, point)?;
        ensure!(
            cell.output.lock == code.output.lock
                && cell.output.type_script.is_none()
                && bytes(&cell.output_data)?.is_empty(),
            "Upgrade funding must be untyped code-authority CKB"
        );
        inputs.push(cell);
    }
    let total = inputs.iter().try_fold(0u64, |n, c| {
        n.checked_add(quantity(&c.output.capacity)?)
            .context("Capacity overflow")
    })?;
    let mut output = code.output.clone();
    let min = withdraw::output_cell(&output, &hx(binary))?
        .minimum_capacity()
        .map_err(|e| anyhow::anyhow!("{e:?}"))?;
    let capacity = min.max(quantity(&code.output.capacity)?);
    output.capacity = format!("0x{capacity:x}");
    let mut outputs = vec![output];
    let mut data = vec![hx(binary)];
    let remainder = total
        .checked_sub(capacity)
        .and_then(|n| n.checked_sub(r.fee_shannons))
        .context("Insufficient external upgrade funding")?;
    if remainder > 0 {
        let change = CellOutput {
            capacity: format!("0x{remainder:x}"),
            lock: code.output.lock.clone(),
            type_script: None,
        };
        ensure!(
            remainder
                >= withdraw::output_cell(&change, "0x")?
                    .minimum_capacity()
                    .map_err(|e| anyhow::anyhow!("{e:?}"))?,
            "Upgrade change below occupied capacity"
        );
        outputs.push(change);
        data.push("0x".into());
    }
    let mut deps = vec![];
    for dep in &r.dependencies {
        ensure!(
            dep.dep_type == "code",
            "Use explicit code dependencies for upgrade"
        );
        let cell = chain::live(rpc, &dep.out_point)?;
        ensure!(
            hx(hash(&bytes(&cell.output_data)?)) == dep.data_hash,
            "Dependency hash mismatch"
        );
        deps.push(cell);
    }
    let mut lock = cfg;
    lock.resize(lock.len() + 65 * lock[2] as usize, 0);
    let first = packed::WitnessArgs::new_builder()
        .lock(Some(gw_types::bytes::Bytes::from(lock)).pack())
        .build();
    let mut witnesses = vec!["0x".into(); inputs.len()];
    witnesses[0] = hx(first.as_slice());
    let tx = Transaction {
        version: "0x0".into(),
        cell_deps: r
            .dependencies
            .iter()
            .map(|d| json!({"out_point":d.out_point,"dep_type":d.dep_type}))
            .collect(),
        header_deps: vec![],
        inputs: inputs
            .iter()
            .map(|c| json!({"since":"0x0","previous_output":c.out_point}))
            .collect(),
        outputs,
        outputs_data: data,
        witnesses,
    };
    withdraw::verify_size(&tx)?;
    ensure!(
        r.fee_shannons >= tx.packed()?.as_slice().len() as u64 + 4,
        "Upgrade fee below 1000 shannons/KB relay baseline"
    );
    let sk = Skeleton {
        request: r.clone(),
        transaction: tx,
        input_cells: inputs,
        dependency_cells: deps,
        profile_hash: d.profile_hash.clone(),
        new_data_hash: d.validator_data_hash.clone(),
        code_type_hash: ty.hash()?,
    };
    signing_request(&sk)?;
    Ok(sk)
}
pub fn signing_request(sk: &Skeleton) -> Result<SigningRequest> {
    let tx = &sk.transaction;
    let r = &sk.request;
    let cfg = config(&r.multisig_config)?;
    ensure!(
        !sk.input_cells.is_empty()
            && tx.input_points()?
                == sk
                    .input_cells
                    .iter()
                    .map(|c| c.out_point.clone())
                    .collect::<Vec<_>>(),
        "Upgrade input resolution mismatch"
    );
    ensure!(
        sk.input_cells
            .iter()
            .skip(1)
            .map(|c| c.out_point.clone())
            .collect::<Vec<_>>()
            == r.funding_inputs,
        "Upgrade funding differs from request"
    );
    ensure!(
        sk.input_cells
            .iter()
            .map(|c| &c.out_point)
            .collect::<BTreeSet<_>>()
            .len()
            == sk.input_cells.len(),
        "Duplicate upgrade input"
    );
    ensure!(
        sk.input_cells
            .iter()
            .skip(1)
            .all(|c| c.output.type_script.is_none() && c.output_data == "0x"),
        "Unexpected typed upgrade funding"
    );
    let lock = &sk.input_cells[0].output.lock;
    ensure!(
        lock.code_hash == hx(c::MULTISIG)
            && lock.hash_type == "type"
            && lock.args == hx(&hash(&cfg)[..20])
            && sk.input_cells.iter().all(|c| c.output.lock == *lock),
        "Upgrade signing lock mismatch"
    );
    ensure!(
        tx.version == "0x0"
            && tx.header_deps.is_empty()
            && tx.outputs.len() >= 1
            && tx.outputs.len() <= 2
            && tx.outputs.len() == tx.outputs_data.len()
            && tx.witnesses.len() == tx.inputs.len(),
        "Unexpected upgrade transaction shape"
    );
    let code = &sk.input_cells[0];
    ensure!(
        code.out_point == r.code_cell && hx(hash(&bytes(&code.output_data)?)) == r.old_data_hash,
        "Wrong original code cell"
    );
    ensure!(
        tx.outputs[0].type_script == code.output.type_script
            && tx.outputs[0].lock == *lock
            && hx(hash(&bytes(&tx.outputs_data[0])?)) == sk.new_data_hash
            && code
                .output
                .type_script
                .as_ref()
                .context("TYPE_ID absent")?
                .hash()?
                == sk.code_type_hash,
        "Upgrade changes code identity or reviewed binary"
    );
    ensure!(
        tx.outputs
            .iter()
            .skip(1)
            .zip(tx.outputs_data.iter().skip(1))
            .all(|(o, data)| o.lock == *lock && o.type_script.is_none() && data == "0x"),
        "Upgrade change redirects funds"
    );
    let ins = sk.input_cells.iter().try_fold(0u64, |n, c| {
        n.checked_add(quantity(&c.output.capacity)?)
            .context("Capacity overflow")
    })?;
    let outs = tx.outputs.iter().try_fold(0u64, |n, c| {
        n.checked_add(quantity(&c.capacity)?)
            .context("Capacity overflow")
    })?;
    ensure!(
        ins.checked_sub(outs) == Some(r.fee_shannons),
        "Upgrade fee mismatch"
    );
    for (o, data) in tx.outputs.iter().zip(&tx.outputs_data) {
        ensure!(
            quantity(&o.capacity)?
                >= withdraw::output_cell(o, data)?
                    .minimum_capacity()
                    .map_err(|e| anyhow::anyhow!("{e:?}"))?,
            "Output under occupied capacity"
        );
    }
    let mut placeholder = cfg.clone();
    placeholder.resize(cfg.len() + 65 * cfg[2] as usize, 0);
    let w = packed::WitnessArgs::from_slice(&bytes(&tx.witnesses[0])?)?;
    ensure!(
        w.lock()
            .to_opt()
            .context("Missing lock witness")?
            .raw_data()
            .as_ref()
            == placeholder,
        "Upgrade signing requires untouched placeholders"
    );
    let mut h = gw_hash::blake2b::new_blake2b();
    h.update(&hash32(&tx.hash()?)?);
    for w in &tx.witnesses {
        let b = bytes(w)?;
        h.update(&(b.len() as u64).to_le_bytes());
        h.update(&b);
    }
    let mut message = [0; 32];
    h.finalize(&mut message);
    Ok(SigningRequest {
        transaction_hash: tx.hash()?,
        transaction_with_placeholders_hash: hx(hash(tx.packed()?.as_slice())),
        groups: vec![SigningGroup {
            lock_hash: lock.hash()?,
            input_indices: (0..tx.inputs.len()).collect(),
            kind: "ckb_multisig".into(),
            message: hx(message),
            signing_digest: hx(message),
            required_signatures: cfg[2] as usize,
            require_first_n: cfg[1] as usize,
            identities: cfg[4..].chunks_exact(20).map(hx).collect(),
        }],
    })
}
pub fn assemble(sk: &Skeleton, responses: &[Signatures]) -> Result<Transaction> {
    let request = signing_request(sk)?;
    let group = &request.groups[0];
    let mut sigs = std::collections::BTreeMap::new();
    for response in responses {
        ensure!(
            response.transaction_hash == request.transaction_hash,
            "Signature transaction mismatch"
        );
        for sig in &response.signatures {
            ensure!(sig.lock_hash == group.lock_hash, "Signature lock mismatch");
            let (i, b) = crate::settlement::signing::recovered(group, &sig.signature)?;
            ensure!(sigs.insert(i, b).is_none(), "Duplicate signer");
        }
    }
    ensure!(
        sigs.len() == group.required_signatures
            && (0..group.require_first_n).all(|i| sigs.contains_key(&i)),
        "Upgrade signature threshold not met"
    );
    let mut b = config(&sk.request.multisig_config)?;
    for s in sigs.values() {
        b.extend_from_slice(s)
    }
    let mut tx = sk.transaction.clone();
    let w = packed::WitnessArgs::from_slice(&bytes(&tx.witnesses[0])?)?;
    tx.witnesses[0] = hx(w
        .as_builder()
        .lock(Some(gw_types::bytes::Bytes::from(b)).pack())
        .build()
        .as_slice());
    Ok(tx)
}
pub fn verify(
    sk: &Skeleton,
    d: &Deployment,
    tx: &Transaction,
    rpc: &impl Chain,
) -> Result<serde_json::Value> {
    // Rebuild from live cells and the reviewed release, not from untrusted explanatory fields.
    let binary = bytes(
        sk.transaction
            .outputs_data
            .first()
            .context("Code data absent")?,
    )?;
    let fresh = build(&sk.request, d, &binary, rpc)?;
    ensure!(
        serde_json::to_value(&fresh)? == serde_json::to_value(sk)?,
        "Stale or altered upgrade skeleton"
    );
    ensure!(
        tx.witnesses.len() == sk.transaction.witnesses.len(),
        "Upgrade witness count changed"
    );
    let mut normalized = tx.clone();
    normalized.witnesses = sk.transaction.witnesses.clone();
    ensure!(
        serde_json::to_value(&normalized)? == serde_json::to_value(&sk.transaction)?,
        "Signed upgrade changed transaction fields"
    );
    let result = rpc.call("estimate_cycles", json!([tx]))?;
    let cycles = quantity(result["cycles"].as_str().context("Missing VM cycles")?)?;
    ensure!(cycles <= 70_000_000, "Upgrade exceeds cycle budget");
    Ok(
        json!({"status":"verified_not_broadcast","transaction_hash":tx.hash()?,"cycles":cycles,"new_data_hash":sk.new_data_hash,"code_type_hash":sk.code_type_hash}),
    )
}
