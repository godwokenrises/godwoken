#![cfg(feature = "vm-tests")]
mod support;
use ckb_testtool::{
    ckb_chain_spec::consensus::ConsensusBuilder,
    ckb_script::{ScriptGroupType, TransactionScriptsVerifier, TxVerifyEnv},
    ckb_types::{
        bytes::Bytes,
        core::{
            cell::{CellMetaBuilder, ResolvedTransaction},
            hardfork::{HardForks, CKB2021, CKB2023},
            HeaderBuilder, TransactionBuilder,
        },
        packed as c,
        prelude::*,
    },
    context::Context,
};
use exit_withdraw_state_validator::*;
use gw_types::prelude::Entity as GwEntity;
use std::sync::Arc;
use support::*;

// CKB v0.200.0 default tx-pool admission limit. This is not the block limit.
const TX_POOL_CYCLES: u64 = 70_000_000;
// Reserve 10M for bootstrap/multisig and sUDT groups, which this harness does not run.
const RECOVERY_AND_CUSTODIAN_BUDGET: u64 = 60_000_000;

#[test]
fn export_test_profile() {
    let f = Fixture::new();
    let data = encode_profile(&f.p);
    if let Ok(path) = std::env::var("GW_EXIT_TEST_PROFILE") {
        std::fs::write(&path, &data).unwrap();
        std::fs::write(
            format!("{path}.hash"),
            hex::encode(gw_hash::blake2b::hash(&data)),
        )
        .unwrap();
    }
    println!(
        "test profile hash: {}",
        hex::encode(gw_hash::blake2b::hash(&data))
    );
}
fn output(cell: &Cell) -> c::CellOutput {
    c::CellOutput::new_builder()
        .capacity(cell.capacity.pack())
        .lock(c::Script::from_slice(cell.lock.as_slice()).unwrap())
        .type_(
            cell.type_
                .as_ref()
                .map(|s| c::Script::from_slice(s.as_slice()).unwrap())
                .pack(),
        )
        .build()
}
// Execute only the recovery type group: signature locks are independently enforced by CKB.
// No always-success replacement is deployed under a production lock hash.
fn run(
    f: &Fixture,
    action: Vec<u8>,
    profile: Option<Vec<u8>>,
    config: bool,
) -> std::result::Result<u64, String> {
    let path = std::env::var("GW_EXIT_VM_BINARY")
        .expect("build fixture-bound binary and set GW_EXIT_VM_BINARY");
    let binary = std::fs::read(path).unwrap();
    let mut ctx = Context::default();
    let code = c::OutPoint::new_builder()
        .tx_hash([200u8; 32].pack())
        .build();
    ctx.cells.insert(
        code.clone(),
        (
            c::CellOutput::new_builder()
                .type_(Some(c::Script::from_slice(code_type().as_slice()).unwrap()).pack())
                .build(),
            binary.into(),
        ),
    );
    let mut deps = vec![code];
    let evidence: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../exit-withdraw/tests/fixtures/mainnet-evidence/v1-code-live-custodian_lock.json"
    ))
    .unwrap();
    let live = &evidence["response"]["result"]["cell"];
    let ct = &live["output"]["type"];
    let decode = |x: &serde_json::Value| {
        hex::decode(x.as_str().unwrap().strip_prefix("0x").unwrap()).unwrap()
    };
    let custodian_type = c::Script::new_builder()
        .code_hash(c::Byte32::from_slice(&decode(&ct["code_hash"])).unwrap())
        .hash_type(1u8.into())
        .args(Bytes::from(decode(&ct["args"])).pack())
        .build();
    assert_eq!(
        custodian_type.calc_script_hash().as_slice(),
        custodian_hash()
    );
    let data = decode(&live["data"]["content"]);
    assert_eq!(
        hex::encode(gw_hash::blake2b::hash(&data)),
        "a254fee1ed74ae1e2e9b30df6607b27e6368aa950e5c0b8eaf33267c6c8addb0"
    );
    let op = c::OutPoint::new_builder()
        .tx_hash([201u8; 32].pack())
        .build();
    ctx.cells.insert(
        op.clone(),
        (
            c::CellOutput::new_builder()
                .type_(Some(custodian_type).pack())
                .build(),
            data.into(),
        ),
    );
    deps.push(op);

    if let Some(p) = profile {
        deps.push(ctx.deploy_cell(p.into()));
    }
    if config {
        deps.push(ctx.deploy_cell(f.cfg.as_slice().to_vec().into()));
    }
    let mut inputs = vec![];
    for (cell, op) in f.tx.inputs.iter().zip(&f.tx.outpoints) {
        let op = c::OutPoint::from_slice(op.as_slice()).unwrap();
        ctx.cells
            .insert(op.clone(), (output(cell), cell.data.clone().into()));
        inputs.push(c::CellInput::new_builder().previous_output(op).build());
    }
    let index =
        f.tx.outputs
            .iter()
            .position(|x| {
                x.type_
                    .as_ref()
                    .is_some_and(|s| s.as_slice() == f.p.rollup.as_slice())
            })
            .unwrap_or(0);
    let mut witnesses = vec![Bytes::new(); f.tx.inputs.len().max(index + 1)];
    witnesses[index] = c::WitnessArgs::new_builder()
        .output_type(Some(Bytes::from(action)).pack())
        .build()
        .as_bytes();
    let tx = TransactionBuilder::default()
        .inputs(inputs)
        .outputs(f.tx.outputs.iter().map(output).collect::<Vec<_>>())
        .outputs_data(
            f.tx.outputs
                .iter()
                .map(|x| Bytes::from(x.data.clone()).pack())
                .collect::<Vec<_>>(),
        )
        .cell_deps(
            deps.iter()
                .map(|op| c::CellDep::new_builder().out_point(op.clone()).build())
                .collect::<Vec<_>>(),
        )
        .witnesses(witnesses.into_iter().map(|x| x.pack()).collect::<Vec<_>>())
        .build();
    let meta = |op: &c::OutPoint| {
        let (o, d) = ctx.cells.get(op).unwrap();
        CellMetaBuilder::from_cell_output(o.clone(), d.clone())
            .out_point(op.clone())
            .build()
    };
    let resolved = ResolvedTransaction {
        transaction: tx.clone(),
        resolved_inputs: tx
            .inputs()
            .into_iter()
            .map(|i| meta(&i.previous_output()))
            .collect(),
        resolved_cell_deps: deps.iter().map(meta).collect(),
        resolved_dep_groups: vec![],
    };
    let consensus = ConsensusBuilder::default()
        .hardfork_switch(HardForks {
            ckb2021: CKB2021::new_dev_default(),
            ckb2023: CKB2023::new_dev_default(),
        })
        .build();
    let tip = HeaderBuilder::default().number(0u64.pack()).build();
    let verifier = TransactionScriptsVerifier::new(
        Arc::new(resolved),
        ctx,
        Arc::new(consensus),
        Arc::new(TxVerifyEnv::new_submit(&tip)),
    );
    let cycles = verifier
        .verify_single(
            ScriptGroupType::Type,
            &c::Script::from_slice(f.p.rollup.as_slice())
                .unwrap()
                .calc_script_hash(),
            RECOVERY_AND_CUSTODIAN_BUDGET,
        )
        .map_err(|e| format!("{e:?}"))?;
    let mut total = cycles;
    let mut seen = std::collections::BTreeSet::new();
    for cell in &f.tx.inputs {
        if cell.lock.code_hash().as_slice() != custodian_hash() {
            continue;
        }
        let lock = c::Script::from_slice(cell.lock.as_slice()).unwrap();
        let hash = lock.calc_script_hash();
        if !seen.insert(hash.clone()) {
            continue;
        }
        let remaining = RECOVERY_AND_CUSTODIAN_BUDGET
            .checked_sub(total)
            .ok_or("cycle budget exceeded")?;
        let used = verifier
            .verify_single(ScriptGroupType::Lock, &hash, remaining)
            .map_err(|e| format!("custodian: {e:?}"))?;
        total += used;
    }
    assert!(total <= RECOVERY_AND_CUSTODIAN_BUDGET);
    assert!(total + 10_000_000 <= TX_POOL_CYCLES);
    println!(
        "recovery cycles: {cycles}; all custodian groups: {}; combined: {total}; reserved: {}",
        total - cycles,
        TX_POOL_CYCLES - total
    );
    Ok(cycles)
}
fn valid(f: &Fixture) -> std::result::Result<u64, String> {
    run(
        f,
        encode_action(&f.a),
        Some(encode_profile(&Fixture::new().p)),
        true,
    )
}
fn rejected(result: std::result::Result<u64, String>, e: Error) {
    let msg = result.unwrap_err();
    assert!(
        msg.starts_with("ValidationFailure(") && msg.ends_with(&format!(", {})", e as i8)),
        "{msg}"
    );
}
#[test]
fn vm_valid_bootstrap() {
    let n = valid(&Fixture::new()).unwrap();
    println!("bootstrap cycles: {n}");
}
#[test]
fn vm_continuation() {
    let mut f = Fixture::new();
    f.tx.inputs[0].lock = f.p.governance.clone();
    println!("continuation cycles: {}", valid(&f).unwrap());
}
#[test]
fn vm_missing_profile() {
    let f = Fixture::new();
    rejected(run(&f, encode_action(&f.a), None, true), Error::Profile);
}
#[test]
fn vm_replaced_profile() {
    let f = Fixture::new();
    let mut p = encode_profile(&f.p);
    p[50] ^= 1;
    rejected(run(&f, encode_action(&f.a), Some(p), true), Error::Profile);
}
#[test]
fn vm_missing_config() {
    let f = Fixture::new();
    rejected(
        run(&f, encode_action(&f.a), Some(encode_profile(&f.p)), false),
        Error::Profile,
    );
}
#[test]
fn vm_normal_actions_disabled() {
    let f = Fixture::new();
    for tag in 0u32..4 {
        let mut a = encode_action(&f.a);
        a[..4].copy_from_slice(&tag.to_le_bytes());
        rejected(
            run(&f, a, Some(encode_profile(&f.p)), true),
            Error::Encoding,
        );
    }
}
#[test]
fn vm_bad_omnilock() {
    let mut f = Fixture::new();
    f.tx.outputs[3].lock = eth_lock([55; 20]);
    rejected(valid(&f), Error::Output);
}
#[test]
fn vm_bad_proof() {
    let mut f = Fixture::new();
    f.a.proof.0[0] ^= 255;
    rejected(valid(&f), Error::Proof);
}
#[test]
fn vm_bad_post_root() {
    let mut f = Fixture::new();
    set_root(&mut f.tx.outputs[0], [99; 32]);
    rejected(valid(&f), Error::State);
}
#[test]
fn vm_pool_fee() {
    let mut f = Fixture::new();
    f.tx.outputs[1].capacity -= 1;
    rejected(valid(&f), Error::Conservation);
}
#[test]
fn vm_oversized_witness() {
    let f = Fixture::new();
    rejected(
        run(
            &f,
            vec![0; MAX_ACTION + 4097],
            Some(encode_profile(&f.p)),
            true,
        ),
        Error::Limit,
    );
}
#[test]
fn vm_output_witness_index() {
    let mut f = Fixture::new();
    f.tx.outputs.swap(0, 5);
    valid(&f).unwrap();
}

#[test]
fn vm_second_batch() {
    let mut f = Fixture::new();
    f.advance();
    println!("second batch cycles: {}", valid(&f).unwrap());
}
#[test]
fn vm_replay() {
    let mut f = Fixture::new();
    let old = f.a.clone();
    f.advance();
    f.a = old;
    rejected(valid(&f), Error::Proof);
}
#[test]
fn vm_eight_recipients() {
    let mut f = Fixture::new();
    f.eight_recipients();
    println!("eight recipient cycles: {}", valid(&f).unwrap());
}
#[test]
fn vm_oversized_lock() {
    let mut f = Fixture::new();
    f.tx.inputs[1].lock = script(custodian_hash(), vec![0; MAX_SCRIPT]);
    rejected(valid(&f), Error::Limit);
}
#[test]
fn vm_oversized_cell_data() {
    let mut f = Fixture::new();
    f.tx.inputs[1].data = vec![0; MAX_CELL_DATA + 1];
    rejected(valid(&f), Error::Limit);
}

#[test]
fn vm_explicit_adversarial_matrix() {
    for (name, f, expected) in adversarial::cases() {
        println!("adversarial VM case: {name}");
        rejected(valid(&f), expected);
    }
}

#[test]
fn vm_balanced_extra_custodian() {
    valid(&adversarial::balanced_extra_custodian()).unwrap();
}

#[test]
fn vm_dense_background_state_budget() {
    let mut f = Fixture::new();
    f.add_background_state(32_768);
    println!(
        "dense two-recipient proof bytes: {}; keys: {}",
        f.a.proof.0.len(),
        f.a.kv.len()
    );
    println!("dense two-recipient cycles: {}", valid(&f).unwrap());
    f.eight_recipients();
    println!(
        "dense eight-recipient proof bytes: {}; keys: {}",
        f.a.proof.0.len(),
        f.a.kv.len()
    );
    println!("dense eight-recipient cycles: {}", valid(&f).unwrap());
}

#[test]
fn vm_multiple_assets_budget() {
    let mut f = Fixture::new();
    f.multiple_assets(8);
    println!(
        "eight-token proof bytes: {}; keys: {}",
        f.a.proof.0.len(),
        f.a.kv.len()
    );
    println!("eight-token cycles: {}", valid(&f).unwrap());
}

#[test]
fn vm_maximum_cells_and_all_custodian_groups_budget() {
    let f = maximum_cells_fixture();
    f.verify().unwrap();
    println!("maximum-cell cycles: {}", valid(&f).unwrap());
}

#[test]
fn vm_large_batch_requires_splitting() {
    let mut f = Fixture::new();
    f.multiple_assets(40);
    // Consensus-correct does not imply relayable. The VM budget must catch this.
    f.verify().unwrap();
    let error = valid(&f).unwrap_err();
    assert!(error.contains("ExceededMaximumCycles"), "{error}");
}
