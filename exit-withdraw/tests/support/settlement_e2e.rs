use super::*;
use exit_withdraw_state_validator::settlement::{self as sc, records::*};
use godwoken_withdraw::claim;
use godwoken_withdraw::settlement::{self as tool, plan::Plan};
use gw_types::U256;
pub(super) fn sign(sk: &Skeleton) -> Transaction {
    let mut tx = sk.transaction.clone();
    for g in sk.signing_groups.as_array().unwrap() {
        let indices: Vec<_> = g["input_indices"]
            .as_array()
            .unwrap()
            .iter()
            .map(|i| i.as_u64().unwrap() as usize)
            .collect();
        let bootstrap = g["kind"] == "bootstrap_lock";
        let ethereum =
            bootstrap && bytes(&sk.input_cells[indices[0]].output.lock.args).unwrap()[0] == 1;
        sign_group_with_mode(
            &mut tx,
            &indices,
            if bootstrap { 2 } else { 3 },
            bootstrap,
            ethereum,
        );
    }
    tx
}
fn setup_two(dbpath: &Path) -> Setup {
    let mut b = support::fixture_settlement(dbpath, &[eth(1), eth(4), [0; 20]]);
    let gov = script(contract::MULTISIG, hash(&multisig(3))[..20].to_vec());
    let old: Value = serde_json::from_str(include_str!(
        "../fixtures/mainnet-evidence/eth-lock-research/godwoken-omnilock-live.json"
    ))
    .unwrap();
    let oldcell = &old["result"]["cell"]; // normalized below if the evidence has an outer wrapper
    let oldcell = if oldcell.is_null() {
        &old["response"]["result"]["cell"]
    } else {
        oldcell
    };
    let oldtype: Script = serde_json::from_value(oldcell["output"]["type"].clone()).unwrap();
    let boot = script(
        hash32(&oldtype.hash().unwrap()).unwrap(),
        [
            &[0][..],
            &hash(&PublicKey::from_secret_key(&Secp256k1::new(), &key(2)).serialize())[..20],
            &[0],
        ]
        .concat(),
    );
    b.rollup_cell.output.lock = boot.clone();
    b.rollup_cell.output.capacity = format!("0x{:x}", 800 * CAP);
    b.cells.get_mut("custodian").unwrap()[0].output.capacity = format!("0x{:x}", 2000 * CAP);
    let sponsor = CellOutput {
        capacity: format!("0x{:x}", 100 * CAP),
        lock: gov.clone(),
        type_script: None,
    };
    let mut outputs = vec![b.rollup_cell.output.clone()];
    let mut data = vec![b.rollup_cell.output_data.clone()];
    for c in &b.cells["custodian"] {
        outputs.push(c.output.clone());
        data.push(c.output_data.clone());
    }
    outputs.push(sponsor);
    data.push("0x".into());
    let genesis = base_tx(outputs, data);
    b.rollup_cell = genesis.cell(&point(&genesis, 0)).unwrap();
    for (i, c) in b.cells.get_mut("custodian").unwrap().iter_mut().enumerate() {
        *c = genesis.cell(&point(&genesis, i + 1)).unwrap();
    }
    let cfg = Config::parse(&bytes(&b.config_data).unwrap()).unwrap();
    let mut p = contract::Profile {
        rollup: rollup_script().packed().unwrap(),
        config_hash: hash(&bytes(&b.config_data).unwrap()),
        bootstrap_outpoint: withdraw::outpoint(&b.rollup_cell.out_point).unwrap(),
        bootstrap_capacity: 800 * CAP,
        bootstrap_lock: boot.packed().unwrap(),
        bootstrap_state: packed::GlobalState::from_slice(
            &bytes(&b.rollup_cell.output_data).unwrap(),
        )
        .unwrap(),
        governance: gov.packed().unwrap(),
        change: b.cells["custodian"][0].output.lock.packed().unwrap(),
        eoa_codes: vec![hash32(
            "0x096df264f38fff07f3acd318995abc2c71ae0e504036fe32bc38d5b6037364d4",
        )
        .unwrap()],
        catalogue: vec![
            (1, [0; 32]),
            (
                3,
                hash32(
                    &b.cells["custodian"][1]
                        .output
                        .type_script
                        .as_ref()
                        .unwrap()
                        .hash()
                        .unwrap(),
                )
                .unwrap(),
            ),
        ],
    };
    for n in 0..46 {
        p.catalogue.push((7 + n, [(128 + n) as u8; 32]));
    }
    let profile = contract::codec::encode_profile(&p);
    let mut node = Node::default();
    node.insert(genesis.clone());
    let mut deps = vec![];
    let mut deploy = |typ: Option<Script>, data: Vec<u8>| {
        let tx = base_tx(
            vec![CellOutput {
                capacity: format!("0x{:x}", 100_000_000 * CAP),
                lock: gov.clone(),
                type_script: typ,
            }],
            vec![hx(&data)],
        );
        let op = point(&tx, 0);
        node.insert(tx);
        deps.push(Dependency {
            out_point: op,
            dep_type: "code".into(),
            data_hash: hx(hash(&data)),
        });
    };
    let evidence: Value = serde_json::from_str(include_str!(
        "../fixtures/mainnet-evidence/v1-code-live-state_validator.json"
    ))
    .unwrap();
    let validator_type: Script =
        serde_json::from_value(evidence["response"]["result"]["cell"]["output"]["type"].clone())
            .unwrap();
    let binary = std::env::var("GW_WITHDRAW_E2E_BINARY")
        .ok()
        .map(|p| std::fs::read(p).unwrap())
        .unwrap_or_else(|| vec![0]);
    let binaryhash = hx(hash(&binary));
    deploy(Some(validator_type), binary);
    deploy(None, profile.clone());
    deploy(None, bytes(&b.config_data).unwrap());
    let custody: Value = serde_json::from_str(include_str!(
        "../fixtures/mainnet-evidence/v1-code-live-custodian_lock.json"
    ))
    .unwrap();
    let c = &custody["response"]["result"]["cell"];
    deploy(
        Some(serde_json::from_value(c["output"]["type"].clone()).unwrap()),
        bytes(c["data"]["content"].as_str().unwrap()).unwrap(),
    );
    deploy(
        Some(oldtype),
        bytes(oldcell["data"]["content"].as_str().unwrap()).unwrap(),
    );
    let standard: Value = serde_json::from_str(include_str!(
        "../fixtures/mainnet-evidence/eth-lock-research/standard-omnilock-live.json"
    ))
    .unwrap();
    let c = &standard["result"]["cell"];
    deploy(
        Some(serde_json::from_value(c["output"]["type"].clone()).unwrap()),
        bytes(c["data"]["content"].as_str().unwrap()).unwrap(),
    );
    let sudt: Value =
        serde_json::from_str(include_str!("../fixtures/mainnet-sudt-live.json")).unwrap();
    let c = &sudt["result"]["cell"];
    let typ: Script = serde_json::from_value(c["output"]["type"].clone()).unwrap();
    assert_eq!(typ.hash().unwrap(), cfg.hashes["l1_sudt"]);
    deploy(
        Some(typ),
        bytes(c["data"]["content"].as_str().unwrap()).unwrap(),
    );
    let m: Value =
        serde_json::from_str(include_str!("../fixtures/mainnet-multisig-type.json")).unwrap();
    let typ: Script = serde_json::from_value(m["type"].clone()).unwrap();
    assert_eq!(typ.hash().unwrap(), hx(contract::MULTISIG));
    deploy(
        Some(typ),
        ckb_system_scripts::BUNDLED_CELL
            .get("specs/cells/secp256k1_blake160_multisig_all")
            .unwrap()
            .to_vec(),
    );
    deploy(
        None,
        ckb_system_scripts::BUNDLED_CELL
            .get("specs/cells/secp256k1_data")
            .unwrap()
            .to_vec(),
    );
    Setup {
        d: Deployment {
            profile: hx(&profile),
            profile_hash: hx(hash(&profile)),
            validator_data_hash: binaryhash,
            cell_deps: deps,
            sponsor_inputs: vec![point(&genesis, 3)],
            multisig_config: hx(multisig(3)),
            bootstrap_lock_placeholder: Some(hx(omni_signature(&[0; 65]))),
            fee_shannons: 100_000,
            confirmations: 10,
        },
        b,
        node,
    }
}
pub(super) fn fixture(path: &Path) -> (Setup, Plan) {
    fixture_split(path, false)
}
fn fixture_split(path: &Path, split: bool) -> (Setup, Plan) {
    let mut setup = setup_two(&path.join("db"));
    let db = Database::open(&path.join("db")).unwrap();
    let tables = path.join("tables");
    std::fs::create_dir(&tables).unwrap();
    let mut sources = vec![];
    let mut allocations = vec![];
    for account in [4, 5, 6] {
        for asset in [1, 3] {
            let total = if asset == 1 {
                U256::from(1400) * U256::exp10(18) + 123
            } else {
                500.into()
            };
            sources.push(json!({"source_account":account,"asset_id":asset,"input_raw":total.to_string(),"allocated_and_reserved_raw":total.to_string(),"pass":true}));
            let eligible = if split && (account == 4 || account == 5) {
                if asset == 1 {
                    U256::from(250) * U256::exp10(18)
                        + U256::from(if account == 4 {
                            6_000_000_000u64
                        } else {
                            4_000_000_000u64
                        })
                } else {
                    U256::from(if account == 4 { 10 } else { 90 })
                }
            } else if account == 5 {
                if asset == 1 {
                    U256::from(500) * U256::exp10(18)
                } else {
                    100.into()
                }
            } else {
                0.into()
            };
            let deferred = if account == 5 {
                if asset == 1 {
                    U256::from(100) * U256::exp10(18)
                } else {
                    50.into()
                }
            } else {
                0.into()
            };
            for (status, n) in [
                ("CALCULATED_EOA", eligible),
                ("HOLD_LENDING_CLOSEOUT", deferred),
                ("RESERVED_PROTOCOL_OR_ROUNDING", total - eligible - deferred),
            ] {
                if !n.is_zero() {
                    allocations.push(json!({"source_account":account,"asset_id":asset,"beneficiary":hx(eth(1)),"beneficiary_account_id":4,"quantity_raw":n.to_string(),"status":status,"path":"synthetic protocol source to beneficiary"}));
                }
            }
        }
    }
    let base = setup.d.profile().unwrap();
    let assets:Vec<_>=base.catalogue.iter().map(|(id,h)|json!({"asset_id":id,"type_hash":hx(h),"eligible_before_capacity_raw":if *id==1{(U256::from(600)*U256::exp10(18)).to_string()}else if *id==3{"150".into()}else{"0".into()},"custodian_covers_eligible_before_capacity":true,"l2_covers_eligible_before_capacity":true})).collect();
    for (name, value) in [
        ("summary.json", json!({"snapshot_root": db.root()})),
        ("source-checks.json", json!(sources)),
        ("source-allocations.json", json!(allocations)),
        ("assets.json", json!(assets)),
    ] {
        std::fs::write(tables.join(name), serde_json::to_vec(&value).unwrap()).unwrap();
    }
    let mut plan = tool::plan::export(&db, &tables, [0xa1; 32], [0xb2; 32], &setup.b).unwrap();
    if std::env::var_os("GW_CONTROL_TEST").is_some() {
        let mut owners = vec![eth(1), eth(2), eth(4)];
        owners.sort();
        let descriptor =
            contract::control::descriptor(contract::script_hash(&base.rollup), 4, 2, &owners)
                .unwrap();
        plan.recipient_scripts
            .insert(4, Script::from_packed(descriptor.clone()).unwrap());
        for raw in &mut plan.claims {
            let mut c: Claim = tool::plan::parse(raw).unwrap();
            c.address = contract::control::identity(&descriptor);
            c.script_hash = contract::script_hash(&descriptor);
            *raw = hx(encode(&c));
        }
        plan.claims_root = tool::plan::root(&plan.records().unwrap()[2]).unwrap();
        plan.manifest.clear();
        plan.manifest = hx(hash(&serde_json::to_vec(&plan).unwrap()));
        plan.verify().unwrap();
    }
    let profile = sc::SettlementProfile {
        base,
        deployment: hash32(&plan.deployment).unwrap(),
        manifest: hash32(&plan.manifest).unwrap(),
        source_root: hash32(&plan.source_root).unwrap(),
        source_count: plan.sources.len() as u64,
        reserve_root: hash32(&plan.reserve_root).unwrap(),
        reserve_count: plan.reserves.len() as u64,
        initial_epoch: plan.epoch().unwrap(),
        limits: plan
            .limits
            .iter()
            .map(|s| tool::plan::parse(s).unwrap())
            .collect(),
    };
    let bytes = sc::codec::encode_profile(&profile);
    let old_hash = setup.d.profile_hash.clone();
    let tx = base_tx(
        vec![CellOutput {
            capacity: format!("0x{:x}", 100_000_000 * CAP),
            lock: Script::from_packed(profile.base.governance.clone()).unwrap(),
            type_script: None,
        }],
        vec![hx(&bytes)],
    );
    let dep = setup
        .d
        .cell_deps
        .iter_mut()
        .find(|d| d.data_hash == old_hash)
        .unwrap();
    dep.out_point = point(&tx, 0);
    dep.data_hash = hx(hash(&bytes));
    setup.node.insert(tx);
    setup.d.profile = hx(&bytes);
    setup.d.profile_hash = hx(hash(&bytes));
    (setup, plan)
}
pub(super) fn advance_settlement(s: &mut Setup, tx: Transaction) {
    let change = Script::from_packed(tool::profile(&s.d).unwrap().base.change).unwrap();
    let consumed = tx.input_points().unwrap();
    let mut custodians: Vec<_> = s.b.cells["custodian"]
        .iter()
        .filter(|c| !consumed.contains(&c.out_point))
        .cloned()
        .collect();
    custodians.extend(
        tx.outputs
            .iter()
            .enumerate()
            .filter(|(_, c)| c.lock == change)
            .map(|(i, _)| tx.cell(&point(&tx, i)).unwrap()),
    );
    s.b.cells.insert("custodian".into(), custodians);
    s.b.rollup_cell = tx.cell(&point(&tx, 0)).unwrap();
    let gov = Script::from_packed(tool::profile(&s.d).unwrap().base.governance).unwrap();
    s.d.sponsor_inputs = tx
        .outputs
        .iter()
        .enumerate()
        .filter(|(_, o)| o.lock == gov && o.type_script.is_none())
        .map(|(i, _)| point(&tx, i))
        .collect();
    s.node.insert(tx);
}
pub(super) fn active(s: &mut Setup, plan: &Plan, db: &Database, vm: bool) -> Vec<u64> {
    let mut cycles = vec![];
    for _ in 0..30 {
        let profile = tool::profile(&s.d).unwrap();
        let cfg = Config::parse(&bytes(&s.b.config_data).unwrap()).unwrap();
        let (_, ledger, _) = tool::replay::reconstruct(
            db,
            &s.node,
            &profile,
            plan,
            &cfg.packed,
            &s.b.rollup_cell,
            10,
        )
        .unwrap();
        if ledger.metadata.is_some_and(|m| m.phase == 2) {
            return cycles;
        }
        let sk = tool::prepare(db, &s.node, &s.d, &s.b, plan, None, 2).unwrap();
        let tx = sign(&sk);
        if vm {
            cycles.push(s.node.vm(&tx).unwrap());
        }
        advance_settlement(s, tx);
    }
    panic!("registration did not converge")
}
#[test]
fn export_settlement_profile() {
    let t = tempfile::tempdir().unwrap();
    let (s, p) = fixture(t.path());
    p.verify().unwrap();
    if let Ok(path) = std::env::var("GW_SETTLEMENT_TEST_PROFILE") {
        std::fs::write(&path, bytes(&s.d.profile).unwrap()).unwrap();
        std::fs::write(
            format!("{path}.hash"),
            s.d.profile_hash.trim_start_matches("0x"),
        )
        .unwrap();
    }
}
#[test]
fn settlement_lifecycle_and_adversarial_matrix() {
    let t = tempfile::tempdir().unwrap();
    let (mut s, plan) = fixture(t.path());
    let db = Database::open(&t.path().join("db")).unwrap();
    active(&mut s, &plan, &db, false);
    let profile = tool::profile(&s.d).unwrap();
    let cfg = Config::parse(&bytes(&s.b.config_data).unwrap()).unwrap();
    let sk = tool::prepare(&db, &s.node, &s.d, &s.b, &plan, Some(&[hx(eth(1))]), 2).unwrap();
    let tx = withdraw::chain::resolved(&sk.transaction, &sk.input_cells).unwrap();
    let a = tool::replay::action(&sk.transaction, &profile.base).unwrap();
    let ph = hash32(&s.d.profile_hash).unwrap();
    let verify = |tx: &contract::Transaction, a: &sc::SettlementAction| {
        sc::verify(&profile, ph, &cfg.packed, tx, a)
    };
    assert!(verify(&tx, &a).is_ok());
    let check = |name: &str, tx: contract::Transaction, a: sc::SettlementAction| {
        assert!(verify(&tx, &a).is_err(), "accepted {name}");
    };
    let mut bad = a.clone();
    bad.records.push(bad.records[0].clone());
    check("duplicate claim", tx.clone(), bad);
    let mut bad = a.clone();
    let mut pay: Payment = decode(&bad.records[0]).unwrap();
    pay.claim.address = eth(4);
    bad.records[0] = encode(&pay);
    check("wrong ETH beneficiary", tx.clone(), bad);
    let mut bad = a.clone();
    let mut pay: Payment = decode(&bad.records[0]).unwrap();
    pay.amount = pay.claim.cap + 1;
    bad.records[0] = encode(&pay);
    check("overdraw claim cap", tx.clone(), bad);
    let mut bad = a.clone();
    let mut pay: Payment = decode(&bad.records[0]).unwrap();
    pay.claim.cap += U256::one();
    bad.records[0] = encode(&pay);
    check("forged cap", tx.clone(), bad);
    let mut bad = a.clone();
    bad.native.recipients.push(bad.native.recipients[0].clone());
    check("duplicate ETH recipient", tx.clone(), bad);
    let mut bad = tx.clone();
    let mut args = bad.outputs[1].lock.args().raw_data().to_vec();
    args.push(0);
    bad.outputs[1].lock = bad.outputs[1]
        .lock
        .clone()
        .as_builder()
        .args(args.pack())
        .build();
    check("extra OmniLock args", bad, a.clone());
    let mut bad = tx.clone();
    bad.outputs[1].lock = contract::eth_lock(eth(4));
    check("wrong output ETH", bad, a.clone());
    let mut bad = tx.clone();
    bad.outputs[1].data = 101u128.to_le_bytes().to_vec();
    check("wrong token amount", bad, a.clone());
    let mut bad = tx.clone();
    bad.outputs[1].type_ = Some(profile.base.rollup.clone());
    check("wrong sUDT", bad, a.clone());
    let mut bad = tx.clone();
    bad.outputs.remove(1);
    check("missing beneficiary output", bad, a.clone());
    let mut bad = tx.clone();
    bad.outputs.push(tx.outputs[1].clone());
    check("extra unlisted output", bad, a.clone());
    let mut bad = tx.clone();
    bad.inputs.push(tx.inputs[1].clone());
    bad.outpoints.push(packed::OutPoint::default());
    check("extra custodian input", bad, a.clone());
    let mut bad = tx.clone();
    bad.outputs.push(tx.outputs[2].clone());
    check("extra custodian output", bad, a.clone());
    let mut bad = tx.clone();
    bad.outputs[1].capacity = 1;
    check("insufficient capacity", bad, a.clone());
    let mut bad = a.clone();
    bad.after[4] ^= 1;
    check("forged settlement commitment", tx.clone(), bad);
    let mut bad = a.clone();
    bad.native.proof.0[0] ^= 1;
    check("native proof corruption", tx.clone(), bad);
    let mut bad = a.clone();
    bad.trees[0].proof.0[0] ^= 1;
    check("allocation proof corruption", tx.clone(), bad);
    let mut bad = a.clone();
    bad.before = vec![];
    check("paid-state reset", tx.clone(), bad);
    let mut bad = a.clone();
    bad.op = sc::Operation::Begin;
    check("repeat bootstrap", tx.clone(), bad);
    let mut bad = tx.clone();
    bad.inputs[0].data.push(0);
    check("expanded GlobalState", bad, a.clone());
    assert!(sc::codec::decode_action(&contract::codec::encode_action(&a.native)).is_err());
    let original_root = db.root();
    let payout = sign(&sk);
    advance_settlement(&mut s, payout);
    let (native, ledger, _) = tool::replay::reconstruct(
        &db,
        &s.node,
        &profile,
        &plan,
        &cfg.packed,
        &s.b.rollup_cell,
        10,
    )
    .unwrap();
    assert_eq!(
        contract::amount(native.read(contract::balance_key(1, eth(1))).unwrap()),
        U256::from(1400) * U256::exp10(18) + 123
    );
    assert_eq!(
        contract::amount(native.read(contract::balance_key(1, eth(4))).unwrap()),
        U256::from(900) * U256::exp10(18) + 123
    );
    assert_eq!(
        contract::amount(native.read(contract::balance_key(3, eth(4))).unwrap()),
        400.into()
    );
    for raw in ledger.records[sc::CLAIM_PLAN].values() {
        let c: Claim = decode(raw).unwrap();
        assert_eq!(ledger.paid(profile.deployment, &c).unwrap(), c.cap);
    }
    assert!(tool::prepare(&db, &s.node, &s.d, &s.b, &plan, Some(&[hx(eth(1))]), 2).is_err());
    assert_eq!(db.root(), original_root);
    s.node.orphan = true;
    assert!(tool::replay::reconstruct(
        &db,
        &s.node,
        &profile,
        &plan,
        &cfg.packed,
        &s.b.rollup_cell,
        10
    )
    .is_err());
}
#[test]
fn settlement_full_signed_vm_and_user_omnilock() {
    assert!(
        std::env::var_os("GW_WITHDRAW_E2E_BINARY").is_some(),
        "Run test-settlement-e2e.sh"
    );
    let t = tempfile::tempdir().unwrap();
    let (mut s, plan) = fixture(t.path());
    let db = Database::open(&t.path().join("db")).unwrap();
    let cycles = active(&mut s, &plan, &db, true);
    let sk = tool::prepare(&db, &s.node, &s.d, &s.b, &plan, Some(&[hx(eth(1))]), 2).unwrap();
    let tx = sign(&sk);
    let checked = tool::verify_signed(&db, &s.node, &s.d, &s.b, &plan, &tx, 70_000_000).unwrap();
    let payout_cycles = checked["cycles"].as_u64().unwrap();
    advance_settlement(&mut s, tx.clone());
    let payout = point(&tx, 1);
    let cell = s.node.live[&payout].clone();
    let mut claim = base_tx(vec![cell.output.clone()], vec![cell.output_data.clone()]);
    claim.outputs[0].lock = script(contract::MULTISIG, hash(&multisig(5))[..20].to_vec());
    claim.outputs[0].capacity =
        format!("0x{:x}", quantity(&cell.output.capacity).unwrap() - 100_000);
    claim.inputs = vec![json!({"since":"0x0","previous_output":payout})];
    claim.cell_deps = tx.cell_deps.clone();
    claim.witnesses = vec![hx(packed::WitnessArgs::new_builder()
        .lock(Some(gw_types::bytes::Bytes::from(omni_signature(&[0; 65]))).pack())
        .build()
        .as_slice())];
    let mut wrong = claim.clone();
    sign_group(&mut wrong, &[0], 4, true);
    assert!(s.node.vm(&wrong).is_err());
    sign_group(&mut claim, &[0], 1, true);
    let claim_cycles = s.node.vm(&claim).unwrap();
    let mut altered = claim.clone();
    altered.outputs[0].capacity =
        format!("0x{:x}", quantity(&cell.output.capacity).unwrap() - 200_000);
    assert!(s.node.vm(&altered).is_err());
    s.node.insert(claim.clone());
    assert!(s.node.vm(&claim).is_err());
    println!(
        "settlement cycles: admin={cycles:?}, payout={payout_cycles}, user_claim={claim_cycles}"
    );
}

// Exercise lifecycle invariants using canonical proofs, rather than only corrupting bytes.
#[test]
fn root_initialization_rejects_registration_and_epoch_mutation() {
    let t = tempfile::tempdir().unwrap();
    let (mut setup, plan) = fixture(t.path());
    let db = Database::open(&t.path().join("db")).unwrap();
    active(&mut setup, &plan, &db, false);
    let p = tool::profile(&setup.d).unwrap();
    let cfg = Config::parse(&bytes(&setup.b.config_data).unwrap()).unwrap();
    let (native, ledger, journal) = tool::replay::reconstruct(
        &db,
        &setup.node,
        &p,
        &plan,
        &cfg.packed,
        &setup.b.rollup_cell,
        10,
    )
    .unwrap();
    assert_eq!(journal.len(), 1);
    assert!(ledger.records[sc::SOURCES].is_empty());
    assert!(ledger.records[sc::RESERVES].is_empty());
    assert!(ledger.records[sc::PAID].is_empty());
    for op in [
        sc::Operation::RegisterSources,
        sc::Operation::RegisterReserves,
        sc::Operation::FinalizeSources,
        sc::Operation::SealBudgets,
        sc::Operation::RegisterClaims,
        sc::Operation::Activate,
        sc::Operation::BeginEpoch,
        sc::Operation::Unregister,
        sc::Operation::Abort,
        sc::Operation::Begin,
    ] {
        assert!(ledger
            .build_action(
                &db,
                &native,
                &p,
                hash32(&setup.d.profile_hash).unwrap(),
                op,
                0,
                vec![],
                tool::state::empty_action()
            )
            .is_err());
    }
}

#[test]
fn partial_claim_then_remainder_never_debits_twice() {
    let t = tempfile::tempdir().unwrap();
    let (mut setup, plan) = fixture(t.path());
    let db = Database::open(&t.path().join("db")).unwrap();
    active(&mut setup, &plan, &db, false);
    let p = tool::profile(&setup.d).unwrap();
    let cfg = Config::parse(&bytes(&setup.b.config_data).unwrap()).unwrap();
    let (native, ledger, journal) = tool::replay::reconstruct(
        &db,
        &setup.node,
        &p,
        &plan,
        &cfg.packed,
        &setup.b.rollup_cell,
        10,
    )
    .unwrap();
    let (mut records, mut requests) = tool::select(&ledger, &p, &[hx(eth(1))]).unwrap();
    for b in &mut records {
        let mut pay: Payment = decode(b).unwrap();
        pay.amount /= 2;
        *b = encode(&pay);
    }
    for r in &mut requests {
        if let Request::Amounts { amounts, .. } = r {
            for n in amounts.values_mut() {
                *n = (U256::from_dec_str(n).unwrap() / 2).to_string();
            }
        }
    }
    let sk = tool::build::construct(
        &db,
        &ledger,
        records.clone(),
        0,
        &native,
        &setup.d,
        &setup.b,
        &requests,
        setup
            .d
            .sponsor_inputs
            .iter()
            .map(|p| setup.node.live[p].clone())
            .collect(),
        withdraw::chain::verify_dependencies(&setup.node, &setup.d, &p.base).unwrap(),
        json!(journal),
    )
    .unwrap();
    let tx = sign(&sk);
    advance_settlement(&mut setup, tx.clone());
    let (_, ledger, _) = tool::replay::reconstruct(
        &db,
        &setup.node,
        &p,
        &plan,
        &cfg.packed,
        &setup.b.rollup_cell,
        10,
    )
    .unwrap();
    for raw in records {
        let pay: Payment = decode(&raw).unwrap();
        assert_eq!(
            ledger.paid(p.deployment, &pay.claim).unwrap(),
            pay.claim.cap / 2
        );
    }
    let remainder = tool::prepare(
        &db,
        &setup.node,
        &setup.d,
        &setup.b,
        &plan,
        Some(&[hx(eth(1))]),
        2,
    )
    .unwrap();
    let a = tool::replay::action(&remainder.transaction, &p.base).unwrap();
    for raw in &a.records {
        let pay: Payment = decode(raw).unwrap();
        assert_eq!(pay.amount, pay.claim.cap / 2);
    }
    // The second claim is below the original cap but exceeds the unpaid remainder.
    // It must fail against the paid SMT state, not only against the initial allocation.
    let mut excessive = a.clone();
    let mut pay: Payment = decode(&excessive.records[0]).unwrap();
    pay.amount += U256::one();
    assert!(pay.amount <= pay.claim.cap);
    excessive.records[0] = encode(&pay);
    let resolved =
        withdraw::chain::resolved(&remainder.transaction, &remainder.input_cells).unwrap();
    assert!(sc::verify(
        &p,
        hash32(&setup.d.profile_hash).unwrap(),
        &cfg.packed,
        &resolved,
        &a
    )
    .is_ok());
    assert_eq!(
        sc::verify(
            &p,
            hash32(&setup.d.profile_hash).unwrap(),
            &cfg.packed,
            &resolved,
            &excessive
        )
        .unwrap_err(),
        contract::Error::Balance
    );
    advance_settlement(&mut setup, sign(&remainder));
    assert!(tool::prepare(
        &db,
        &setup.node,
        &setup.d,
        &setup.b,
        &plan,
        Some(&[hx(eth(1))]),
        2
    )
    .is_err());
    assert!(
        tool::verify_signed(&db, &setup.node, &setup.d, &setup.b, &plan, &tx, 70_000_000).is_err()
    );
}

#[test]
fn root_payment_requires_source_and_lot_membership() {
    let t = tempfile::tempdir().unwrap();
    let (mut setup, plan) = fixture(t.path());
    let db = Database::open(&t.path().join("db")).unwrap();
    active(&mut setup, &plan, &db, false);
    let sk = tool::prepare(
        &db,
        &setup.node,
        &setup.d,
        &setup.b,
        &plan,
        Some(&[hx(eth(1))]),
        2,
    )
    .unwrap();
    let p = tool::profile(&setup.d).unwrap();
    let action = tool::replay::action(&sk.transaction, &p.base).unwrap();
    for kind in [
        sc::SOURCE_PLAN,
        sc::LOT_PLAN,
        sc::CLAIM_PLAN,
        sc::SOURCES,
        sc::RESERVES,
        sc::PAID,
    ] {
        let mut bad = action.clone();
        bad.trees.retain(|t| t.kind != kind);
        assert!(
            sc::evaluate(&p, hash32(&setup.d.profile_hash).unwrap(), &bad).is_err(),
            "missing tree {kind}"
        );
        let mut bad = action.clone();
        let t = bad.trees.iter_mut().find(|t| t.kind == kind).unwrap();
        if !t.leaves[0].1.is_empty() {
            t.leaves[0].1[4] ^= 1;
        } else {
            t.leaves[0].1 = vec![1];
        }
        assert!(
            sc::evaluate(&p, hash32(&setup.d.profile_hash).unwrap(), &bad).is_err(),
            "forged leaf {kind}"
        );
    }
}

#[test]
fn settlement_cli_build_sign_verify_and_reconstruct() {
    assert!(
        std::env::var_os("GW_WITHDRAW_E2E_BINARY").is_some(),
        "Run test-settlement-e2e.sh"
    );
    let t = tempfile::tempdir().unwrap();
    let (mut setup, plan) = fixture(t.path());
    let config = setup
        .d
        .cell_deps
        .iter()
        .find(|d| d.data_hash == hx(hash(&bytes(&setup.b.config_data).unwrap())))
        .unwrap()
        .out_point
        .clone();
    let server = Server::new(setup.node.clone());
    let depfile = t.path().join("deployment.json");
    let planfile = t.path().join("plan.json");
    let addresses = t.path().join("addresses.json");
    std::fs::write(&planfile, serde_json::to_vec(&plan).unwrap()).unwrap();
    std::fs::write(&addresses, serde_json::to_vec(&vec![hx(eth(1))]).unwrap()).unwrap();
    let run = |args: Vec<String>| {
        let out = Command::new(env!("CARGO_BIN_EXE_gw-withdraw"))
            .arg("settlement")
            .args(args)
            .env("NO_PROXY", "127.0.0.1")
            .env("no_proxy", "127.0.0.1")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "CLI failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        out
    };
    run(vec![
        "verify-plan".into(),
        "--plan".into(),
        planfile.display().to_string(),
    ]);
    let path = |p: &Path| p.display().to_string();
    let partial = t.path().join("partial-payments.json");
    let selections = plan
        .claims
        .iter()
        .map(|raw| {
            let c: Claim = decode(&bytes(raw).unwrap()).unwrap();
            tool::PaymentSelection {
                claim_key: hx(claim_key(hash32(&plan.deployment).unwrap(), &c)),
                amount_raw: (c.cap / 2).to_string(),
            }
        })
        .collect::<Vec<_>>();
    std::fs::write(&partial, serde_json::to_vec(&selections).unwrap()).unwrap();
    let mut paid = 0;
    let mut observed = vec![];
    for step in 0..32 {
        std::fs::write(&depfile, serde_json::to_vec(&setup.d).unwrap()).unwrap();
        let common = vec![
            "--db".into(),
            path(&t.path().join("db")),
            "--deployment".into(),
            path(&depfile),
            "--plan".into(),
            path(&planfile),
            "--ckb-rpc".into(),
            server.url.clone(),
            "--config-outpoint".into(),
            format!("{}:{}", config.tx_hash, config.index),
        ];
        let output = t.path().join(format!("batch-{step}"));
        let mut args = vec!["build-next".into()];
        args.extend(common.clone());
        args.extend([
            "--batch-size".into(),
            "4".into(),
            "--output-dir".into(),
            path(&output),
        ]);
        if paid == 2 {
            break;
        }
        if observed.iter().any(|o| o == "Begin") {
            if paid == 0 {
                args.extend(["--payments".into(), path(&partial)]);
            } else {
                args.extend(["--addresses".into(), path(&addresses)]);
            }
        }
        run(args);
        let sk: Skeleton =
            serde_json::from_slice(&std::fs::read(output.join("skeleton.json")).unwrap()).unwrap();
        let signing: tool::signing::SigningRequest =
            serde_json::from_slice(&std::fs::read(output.join("signing-request.json")).unwrap())
                .unwrap();
        let mut signatures = vec![];
        for (n, group) in signing.groups.iter().enumerate() {
            let keyfile = t.path().join(format!("fixture-key-{step}-{n}"));
            std::fs::write(
                &keyfile,
                hx(key(if group.kind != "ckb_multisig" { 2 } else { 3 }).secret_bytes()),
            )
            .unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&keyfile, std::fs::Permissions::from_mode(0o600)).unwrap();
            }
            let sigfile = output.join(format!("signature-{n}.json"));
            run(vec![
                "sign-message".into(),
                "--request".into(),
                path(&output.join("signing-request.json")),
                "--lock-hash".into(),
                group.lock_hash.clone(),
                "--private-key-file".into(),
                path(&keyfile),
                "--output".into(),
                path(&sigfile),
            ]);
            signatures.push(path(&sigfile));
        }
        let signedfile = output.join("signed.json");
        let mut args = vec![
            "assemble-signatures".into(),
            "--deployment".into(),
            path(&depfile),
            "--skeleton".into(),
            path(&output.join("skeleton.json")),
            "--output".into(),
            path(&signedfile),
            "--signatures".into(),
        ];
        args.extend(signatures);
        run(args);
        let tx: Transaction = serde_json::from_slice(&std::fs::read(&signedfile).unwrap()).unwrap();
        let mut args = vec!["verify-signed".into()];
        args.extend(common);
        args.extend([
            "--transaction".into(),
            path(&signedfile),
            "--output".into(),
            path(&output.join("verified.json")),
        ]);
        run(args);
        let verified: Value =
            serde_json::from_slice(&std::fs::read(output.join("verified.json")).unwrap()).unwrap();
        assert!(verified["cycles"].as_u64().unwrap() < 70_000_000);
        assert_eq!(verified["broadcast_request"]["method"], "send_transaction");
        assert_eq!(tx.hash().unwrap(), sk.transaction.hash().unwrap());
        let op = verified["operation"].as_str().unwrap().to_string();
        if op == "Pay" {
            paid += 1;
        }
        observed.push(op);
        advance_settlement(&mut setup, tx);
        *server.node.lock().unwrap() = setup.node.clone();
        let next: Deployment = serde_json::from_slice(
            &std::fs::read(output.join("next-deployment-after-confirmation.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(next.sponsor_inputs, setup.d.sponsor_inputs);
    }
    std::fs::write(&depfile, serde_json::to_vec(&setup.d).unwrap()).unwrap();
    let result = run(vec![
        "status".into(),
        "--db".into(),
        path(&t.path().join("db")),
        "--plan".into(),
        path(&planfile),
        "--deployment".into(),
        path(&depfile),
        "--addresses".into(),
        path(&addresses),
        "--ckb-rpc".into(),
        server.url.clone(),
        "--config-outpoint".into(),
        format!("{}:{}", config.tx_hash, config.index),
        "--output".into(),
        path(&t.path().join("status.json")),
    ]);
    let result: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert!(result["epoch0_accounts"]
        .as_array()
        .unwrap()
        .iter()
        .all(|v| v["remaining_raw"] == "0"));
    assert_eq!(paid, 2);
    assert!(observed.contains(&"Begin".into()));
    println!("CLI lifecycle operations: {observed:?}");
}

#[test]
fn signing_rejects_wrong_duplicate_missing_and_mutated_signatures() {
    let t = tempfile::tempdir().unwrap();
    let (setup, plan) = fixture(t.path());
    let db = Database::open(&t.path().join("db")).unwrap();
    let sk = tool::prepare(&db, &setup.node, &setup.d, &setup.b, &plan, None, 2).unwrap();
    let req = tool::signing::requests(&setup.d, &sk).unwrap();
    let responses: Vec<_> = req
        .groups
        .iter()
        .map(|g| {
            tool::signing::sign(
                &req,
                &g.lock_hash,
                &key(if g.kind != "ckb_multisig" { 2 } else { 3 }).secret_bytes(),
            )
            .unwrap()
        })
        .collect();
    let signed = tool::signing::assemble(&setup.d, &sk, &responses).unwrap();
    assert_eq!(signed.hash().unwrap(), sk.transaction.hash().unwrap());
    assert!(tool::signing::assemble(&setup.d, &sk, &responses[..1]).is_err());
    let mut duplicate = responses.clone();
    duplicate.push(responses[0].clone());
    assert!(tool::signing::assemble(&setup.d, &sk, &duplicate).is_err());
    assert!(tool::signing::sign(&req, &req.groups[0].lock_hash, &key(7).secret_bytes()).is_err());
    let mut wrong = responses.clone();
    wrong[0].transaction_hash = hx([9; 32]);
    assert!(tool::signing::assemble(&setup.d, &sk, &wrong).is_err());
    let mut wrong = responses.clone();
    wrong[0].signatures[0].signature = hx([1; 65]);
    assert!(tool::signing::assemble(&setup.d, &sk, &wrong).is_err());
    let mut wrong_fee = setup.d.clone();
    wrong_fee.fee_shannons += 1;
    assert!(tool::signing::requests(&wrong_fee, &sk).is_err());
    let mut changed = sk;
    changed.transaction.outputs[1].capacity = "0x1".into();
    assert!(tool::signing::assemble(&setup.d, &changed, &responses).is_err());
}

#[test]
fn offline_rehearsal_uses_same_builder_and_preserves_source_db() {
    let t = tempfile::tempdir().unwrap();
    let (setup, plan) = fixture(t.path());
    let db = Database::open(&t.path().join("db")).unwrap();
    let root = db.root();
    let out = t.path().join("host-rehearsal");
    let summary =
        tool::simulation::run(&db, &plan, &setup.d, &setup.b, &[hx(eth(1))], 4, &out).unwrap();
    assert_eq!(summary["status"], "host_consensus_simulation_pass");
    assert_eq!(summary["full_vm_executed"], false);
    assert_eq!(
        summary["asset_counters"][0]["paid_raw"],
        (U256::from(500) * U256::exp10(18)).to_string()
    );
    assert_eq!(db.root(), root);
    assert!(tool::simulation::run(&db, &plan, &setup.d, &setup.b, &[hx(eth(1))], 4, &out).is_err());
    let p = tool::profile(&setup.d).unwrap();
    let mut short = setup.b.clone();
    short
        .cells
        .get_mut("custodian")
        .unwrap()
        .retain(|c| c.output.type_script.is_none());
    assert!(tool::coverage(&p, &short, None).is_err());
}

#[test]
fn same_beneficiary_multiple_sources_combines_subshannon_contributions() {
    let t = tempfile::tempdir().unwrap();
    let (mut setup, plan) = fixture_split(t.path(), true);
    let db = Database::open(&t.path().join("db")).unwrap();
    active(&mut setup, &plan, &db, false);
    let p = tool::profile(&setup.d).unwrap();
    let cfg = Config::parse(&bytes(&setup.b.config_data).unwrap()).unwrap();
    let sk = tool::prepare(
        &db,
        &setup.node,
        &setup.d,
        &setup.b,
        &plan,
        Some(&[hx(eth(1))]),
        4,
    )
    .unwrap();
    let a = tool::replay::action(&sk.transaction, &p.base).unwrap();
    assert_eq!(a.records.len(), 4);
    assert_eq!(a.native.recipients.len(), 1);
    let capacity: u64 = a
        .native
        .payouts
        .iter()
        .map(|v| quantity(&sk.transaction.outputs[v.output as usize].capacity).unwrap())
        .sum();
    assert_eq!(capacity, 500 * CAP + 1);
    advance_settlement(&mut setup, sign(&sk));
    let (native, ledger, _) = tool::replay::reconstruct(
        &db,
        &setup.node,
        &p,
        &plan,
        &cfg.packed,
        &setup.b.rollup_cell,
        10,
    )
    .unwrap();
    for b in a.records {
        let payment: Payment = decode(&b).unwrap();
        assert_eq!(
            ledger.paid(p.deployment, &payment.claim).unwrap(),
            payment.claim.cap
        );
    }
    assert_eq!(
        contract::amount(native.read(contract::balance_key(1, eth(1))).unwrap()),
        U256::from(1150) * U256::exp10(18) + 123 - U256::from(6_000_000_000u64)
    );
    assert_eq!(
        contract::amount(native.read(contract::balance_key(1, eth(4))).unwrap()),
        U256::from(1150) * U256::exp10(18) + 123 - U256::from(4_000_000_000u64)
    );
    assert!(tool::prepare(
        &db,
        &setup.node,
        &setup.d,
        &setup.b,
        &plan,
        Some(&[hx(eth(1)), hx(eth(1))]),
        4
    )
    .is_err());
}

#[test]
fn settlement_refund_only_before_bootstrap_and_after_activation() {
    use crate::refund_shutdown::*;
    assert!(
        std::env::var_os("GW_WITHDRAW_E2E_BINARY").is_some(),
        "Run test-settlement-e2e.sh"
    );
    let t = tempfile::tempdir().unwrap();
    let (mut setup, plan) = fixture(t.path());
    let db = Database::open(&t.path().join("db")).unwrap();
    let addresses = [hx(eth(1))];
    for before_bootstrap in [true, false] {
        if !before_bootstrap {
            active(&mut setup, &plan, &db, false);
        }
        let mut sk = tool::prepare(
            &db,
            &setup.node,
            &setup.d,
            &setup.b,
            &plan,
            if before_bootstrap {
                None
            } else {
                Some(&addresses)
            },
            2,
        )
        .unwrap();
        let original = sk.transaction.clone();
        assert!(setup.node.vm(&sign(&sk)).is_ok());
        let w = packed::WitnessArgs::from_slice(&bytes(&original.witnesses[0]).unwrap()).unwrap();
        let good = w.output_type().to_opt().unwrap().raw_data();
        for (name, payload) in forbidden_actions(&good) {
            sk.transaction = original.clone();
            set_payload(&mut sk.transaction, &payload);
            assert_type_rejection(
                &setup.node,
                &sign(&sk),
                name,
                Some(contract::Error::Encoding as i8),
            );
        }
        for (name, bad) in frozen_state_attacks(&original) {
            sk.transaction = bad;
            assert_type_rejection(&setup.node, &sign(&sk), name, None);
        }
        sk.transaction = original;
        assert!(setup.node.vm(&sign(&sk)).is_ok());
    }
}

#[test]
fn control_v1_full_vm_partial_remainder_and_wrong_signer() {
    if std::env::var_os("GW_CONTROL_TEST").is_none() {
        return;
    }
    assert!(std::env::var_os("GW_WITHDRAW_E2E_BINARY").is_some());
    let temp = tempfile::tempdir().unwrap();
    let (mut setup, plan) = fixture(temp.path());
    let db = Database::open(&temp.path().join("db")).unwrap();
    active(&mut setup, &plan, &db, true);
    let descriptor = plan.recipient_scripts[&4].packed().unwrap();
    let address = hx(contract::control::identity(&descriptor));
    let fund = base_tx(
        (1..=2)
            .map(|k| CellOutput {
                capacity: format!("0x{:x}", 70 * CAP),
                lock: claim::owner(&hx(eth(k))).unwrap(),
                type_script: None,
            })
            .collect(),
        vec!["0x".into(); 2],
    );
    let mut auth = vec![point(&fund, 0), point(&fund, 1)];
    setup.node.insert(fund);
    let dest = claim::address(&claim::owner(&hx(eth(4))).unwrap()).unwrap();
    for round in 0..2 {
        let profile = tool::adapter::Profile::load(&setup.d).unwrap();
        let (native, ledger, journal) = tool::replay::reconstruct_for(
            &db,
            &setup.node,
            &profile,
            &plan,
            &bytes(&setup.b.config_data).unwrap(),
            &setup.b.rollup_cell,
            10,
        )
        .unwrap();
        let (mut records, mut requests) = tool::select_version(
            &ledger,
            &profile,
            &[address.clone()],
            tool::adapter::Protocol::V1,
        )
        .unwrap();
        if round == 0 {
            let selected = records
                .iter()
                .map(|raw| {
                    let p: Payment = decode(raw).unwrap();
                    tool::PaymentSelection {
                        claim_key: hx(claim_key(profile.deployment, &p.claim)),
                        amount_raw: (p.amount / 2).to_string(),
                    }
                })
                .collect::<Vec<_>>();
            (records, requests) =
                tool::select_payments(&ledger, &profile, tool::adapter::Protocol::V1, &selected)
                    .unwrap();
            let mut duplicate = selected.clone();
            duplicate.push(selected[0].clone());
            assert!(tool::select_payments(
                &ledger,
                &profile,
                tool::adapter::Protocol::V1,
                &duplicate
            )
            .is_err());
            let mut excess = selected.clone();
            excess[0].amount_raw = U256::MAX.to_string();
            assert!(
                tool::select_payments(&ledger, &profile, tool::adapter::Protocol::V1, &excess)
                    .is_err()
            );
        }
        let control = tool::build::ControlSpend {
            account_id: 4,
            destination: claim::parse_address(&dest).unwrap(),
            authorization_cells: auth.iter().map(|p| setup.node.live[p].clone()).collect(),
        };
        let sponsors = setup
            .d
            .sponsor_inputs
            .iter()
            .map(|p| setup.node.live[p].clone())
            .collect();
        let deps =
            withdraw::chain::verify_dependencies(&setup.node, &setup.d, &profile.base).unwrap();
        let sk = tool::build::construct_with_controls(
            &db,
            &ledger,
            records,
            0,
            &native,
            &setup.d,
            &setup.b,
            &requests,
            sponsors,
            deps,
            json!(journal),
            &[control],
        )
        .unwrap();
        let req = tool::signing::requests(&setup.d, &sk).unwrap();
        let sigs = req
            .groups
            .iter()
            .map(|g| {
                (1..=4)
                    .find_map(|k| {
                        tool::signing::sign(&req, &g.lock_hash, &key(k).secret_bytes()).ok()
                    })
                    .unwrap()
            })
            .collect::<Vec<_>>();
        assert!(tool::signing::assemble(&setup.d, &sk, &sigs[..sigs.len() - 1]).is_err());
        let mut dup = sigs.clone();
        dup.push(sigs[0].clone());
        assert!(tool::signing::assemble(&setup.d, &sk, &dup).is_err());
        let tx = tool::signing::assemble(&setup.d, &sk, &sigs).unwrap();
        if round == 0 {
            control_api_rehearsal(&setup, &plan, &sk, &req, &sigs, &temp.path().join("tables"));
        }

        if round == 0 {
            if let Ok(path) = std::env::var("GW_CONTROL_BROWSER_FIXTURE") {
                let ops = sigs
                    .iter()
                    .filter(|s| {
                        req.groups.iter().any(|g| {
                            g.kind != "ethereum_personal_sign"
                                && s.signatures.iter().any(|sig| sig.lock_hash == g.lock_hash)
                        })
                    })
                    .collect::<Vec<_>>();
                let approvals = sigs
                    .iter()
                    .filter(|s| {
                        req.groups.iter().any(|g| {
                            g.kind == "ethereum_personal_sign"
                                && s.signatures.iter().any(|sig| sig.lock_hash == g.lock_hash)
                        })
                    })
                    .collect::<Vec<_>>();
                let p = json!({"protocol":"v1","plan_manifest":plan.manifest,"control_accounts":[4],"proposal":{"protocol":"v1","plan_manifest":plan.manifest,"skeleton":sk,"signatures":ops},"signing_request":req});
                std::fs::write(path,serde_json::to_vec_pretty(&json!({"response":p,"approvals":approvals,"owner":hx(eth(1)),"owners":[hx(eth(1)),hx(eth(2)),hx(eth(4))]})).unwrap()).unwrap();
            }
        }

        let mut wrong = tx.clone();
        let g = req
            .groups
            .iter()
            .find(|g| g.kind == "ethereum_personal_sign")
            .unwrap();
        wrong.witnesses[g.input_indices[0]] = sk.transaction.witnesses[g.input_indices[0]].clone();
        assert!(setup.node.vm(&wrong).is_err());
        let mut redirect = tx.clone();
        redirect.outputs[1].lock = claim::owner(&hx(eth(2))).unwrap();
        assert!(setup.node.vm(&redirect).is_err());
        let cycles = setup.node.vm(&tx).unwrap();
        assert!(cycles < 70_000_000);
        println!("V1 control round {round}: {cycles} cycles");
        auth = tx
            .outputs
            .iter()
            .enumerate()
            .filter(|(_, o)| {
                [
                    claim::owner(&hx(eth(1))).unwrap(),
                    claim::owner(&hx(eth(2))).unwrap(),
                ]
                .contains(&o.lock)
                    && o.type_script.is_none()
            })
            .map(|(i, _)| point(&tx, i))
            .collect();
        let payout = point(&tx, 1);
        advance_settlement(&mut setup, tx.clone());
        assert!(setup.node.vm(&tx).is_err());
        let (_, _, _, mut assets, mut request) = claim_e2e::setup_claim();
        assets.push(claim::Asset {
            asset_id: 3,
            symbol: Some("TEST".into()),
            decimals: Some(0),
            l1_decimals: Some(0),
            type_hash: setup.node.live[&payout]
                .output
                .type_script
                .as_ref()
                .unwrap()
                .hash()
                .unwrap(),
        });
        request.address = hx(eth(4));
        request.inputs = vec![payout.clone()];
        let quote = claim::construct(
            &request,
            vec![setup.node.live[&payout].clone()],
            &assets,
            &setup.d.cell_deps,
            1000,
        )
        .unwrap();
        let user =
            claim::assemble(&quote, &claim_e2e::signature(&quote.signing_message, 4)).unwrap();
        println!(
            "V1 controller-selected OmniLock onward exit: {} cycles",
            setup.node.vm(&user).unwrap()
        );
        setup.node.insert(user);
    }
    assert!(tool::prepare_with_controls(
        &db,
        &setup.node,
        &setup.d,
        &setup.b,
        &plan,
        Some(&[address]),
        4,
        &[]
    )
    .is_err());
}

#[test]
fn multisig_upgrade_v1() {
    let t = tempfile::tempdir().unwrap();
    let (s, _) = fixture(t.path());
    super::upgrade_e2e::exercise(s.node, s.d);
}
fn control_api_rehearsal(
    setup: &Setup,
    plan: &Plan,
    sk: &Skeleton,
    req: &tool::signing::SigningRequest,
    sigs: &[tool::signing::Signatures],
    tables: &Path,
) {
    let dir = tempfile::tempdir().unwrap();
    let rpc = Server::new(setup.node.clone());
    let write = |name: &str, v: &Value| {
        let p = dir.path().join(name);
        std::fs::write(&p, serde_json::to_vec(v).unwrap()).unwrap();
        p
    };
    let planfile = write("plan.json", &json!(plan));
    let deployment = write("deployment.json", &json!(setup.d));
    let (_, deps, _, _, _) = claim_e2e::setup_claim();
    let depfile = write("dependencies.json", &json!(deps));
    std::fs::write(tables.join("accounts.json"), "[]").unwrap();
    std::fs::write(dir.path().join("index.html"), "Control API test").unwrap();
    let ops = sigs
        .iter()
        .filter(|s| {
            req.groups.iter().any(|g| {
                g.kind != "ethereum_personal_sign"
                    && s.signatures.iter().any(|x| x.lock_hash == g.lock_hash)
            })
        })
        .collect::<Vec<_>>();
    let approvals = sigs
        .iter()
        .filter(|s| {
            req.groups.iter().any(|g| {
                g.kind == "ethereum_personal_sign"
                    && s.signatures.iter().any(|x| x.lock_hash == g.lock_hash)
            })
        })
        .collect::<Vec<_>>();
    std::fs::create_dir(dir.path().join("v1")).unwrap();
    write(
        &format!("v1/{}.json", sk.transaction.hash().unwrap()),
        &json!({"protocol":"v1","plan_manifest":plan.manifest,"skeleton":sk,"signatures":ops}),
    );
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let listen = listener.local_addr().unwrap().to_string();
    drop(listener);
    let origin = format!("http://{listen}");
    struct Stop(std::process::Child);
    impl Drop for Stop {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let _child = Stop(
        std::process::Command::new(env!("CARGO_BIN_EXE_gw-withdraw"))
            .args([
                "claim-serve",
                "--listen",
                &listen,
                "--public-origin",
                &origin,
                "--ckb-rpc",
                &rpc.url,
                "--tables",
                tables.to_str().unwrap(),
                "--plan",
                planfile.to_str().unwrap(),
                "--deployment",
                deployment.to_str().unwrap(),
                "--dependencies",
                depfile.to_str().unwrap(),
                "--web-dir",
                dir.path().to_str().unwrap(),
                "--control-proposals",
                dir.path().to_str().unwrap(),
                "--enable-broadcast",
            ])
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap(),
    );
    for _ in 0..100 {
        if std::net::TcpStream::connect(&listen).is_ok() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let call = |path: &str, body: Option<Value>| {
        let mut cmd = std::process::Command::new("curl");
        cmd.args([
            "-q",
            "-sS",
            "--noproxy",
            "*",
            "--max-time",
            "20",
            "-H",
            &format!("Origin: {origin}"),
            "-H",
            "Content-Type: application/json",
        ]);
        if let Some(body) = body {
            cmd.args(["--data-binary", &body.to_string()]);
        }
        let out = cmd.arg(format!("{origin}{path}")).output().unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        serde_json::from_slice::<Value>(&out.stdout).unwrap()
    };
    let controls = call("/api/v1/controls", None);
    assert_eq!(controls["accounts"][0]["threshold"], 2);
    let path = format!("/api/v1/control-proposal/{}", req.transaction_hash);
    let fetched = call(&path, None);
    assert_eq!(fetched["control_accounts"], json!([4]));
    assert_eq!(
        fetched["signing_request"]["transaction_hash"],
        req.transaction_hash
    );
    let body = json!({"protocol":"v1","plan_manifest":plan.manifest,"transaction_hash":req.transaction_hash,"signatures":approvals});
    let mut bad = body.clone();
    bad["protocol"] = json!("v0");
    assert!(call("/api/v1/control-submit", Some(bad))["error"].is_string());
    let mut bad = body.clone();
    bad["signatures"] = json!([]);
    assert!(call("/api/v1/control-submit", Some(bad))["error"].is_string());
    let result = call("/api/v1/control-submit", Some(body.clone()));
    assert_eq!(result["status"], "submitted", "{result}");
    assert!(call("/api/v1/control-submit", Some(body))["error"].is_string());
    assert!(call(&path, None)["error"].is_string());
}
