//! Synthetic balances and public test keys, actual mainnet v0 locks and shared settlement builder.
use super::*;
use contract::{
    native::{short_key, NativeVersion, RAW_DESTINATION},
    settlement::{self as s, records::*},
    v0,
};
use godwoken_withdraw::settlement::{
    plan::{self, Tree},
    state::{empty_action, Ledger},
};
use gw_common::state::{
    build_account_field_key, build_script_hash_to_account_id_key, GW_ACCOUNT_SCRIPT_HASH_TYPE,
};
use gw_types::U256;

struct Fixture {
    node: Node,
    deps: Vec<Dependency>,
    profile: v0::Profile,
    config: Vec<u8>,
    native: Tree,
    ledger: Ledger,
    accounts: BTreeMap<u32, packed::Script>,
    roll: OutPoint,
    sponsor: OutPoint,
    custody: Vec<OutPoint>,
    token: Script,
}
fn deploy(node: &mut Node, deps: &mut Vec<Dependency>, typ: Option<Script>, data: Vec<u8>) {
    let tx = base_tx(
        vec![CellOutput {
            capacity: format!("0x{:x}", 100_000_000 * CAP),
            lock: script([0; 32], vec![]),
            type_script: typ,
        }],
        vec![hx(&data)],
    );
    deps.push(Dependency {
        out_point: point(&tx, 0),
        dep_type: "code".into(),
        data_hash: hx(hash(&data)),
    });
    node.insert(tx);
}
fn fixture() -> Fixture {
    fixture_scaled(false, false)
}
fn fixture_scaled(large: bool, one_source: bool) -> Fixture {
    let (mut node, mut poa) = super::v0_upgrade::poa_fixture(true, 2);
    let data_type: Script = serde_json::from_value(
        super::v0_upgrade::evidence("poa-data-current")["response"]["result"]["objects"][0]
            ["output"]["type"]
            .clone(),
    )
    .unwrap();
    let data = node
        .live
        .values()
        .find(|c| c.output.type_script.as_ref() == Some(&data_type))
        .unwrap()
        .clone();
    poa.inputs
        .push(json!({"previous_output":data.out_point,"since":"0x0"}));
    let mut cells: Vec<_> = poa
        .input_points()
        .unwrap()
        .iter()
        .map(|p| node.live[p].clone())
        .collect();
    let snap: Value = serde_json::from_slice(
        &std::fs::read("tests/fixtures/settlement/v0-l1-snapshot.json").unwrap(),
    )
    .unwrap();
    let config = bytes(snap["config_data"].as_str().unwrap()).unwrap();
    let cfg = v0::config(&config).unwrap();
    let roll: Script =
        serde_json::from_value(snap["rollup_cell"]["output"]["type"].clone()).unwrap();
    let roll_hash = hash32(&roll.hash().unwrap()).unwrap();
    let gov = script(
        v0::SECP,
        hash(&PublicKey::from_secret_key(&Secp256k1::new(), &key(3)).serialize())[..20].to_vec(),
    );
    let custody_cell: Cell = serde_json::from_value(snap["cells"]["custodian"][0].clone()).unwrap();
    let token = custody_cell.output.type_script.clone().unwrap();
    let th = hash32(&token.hash().unwrap()).unwrap();
    let mut accounts = BTreeMap::new();
    for (id, t) in [(1, [0; 32]), (3, th)] {
        accounts.insert(
            id,
            script(
                cfg.l2_sudt_validator_script_type_hash().unpack(),
                [roll_hash.as_slice(), &t].concat(),
            )
            .packed()
            .unwrap(),
        );
    }
    accounts.insert(
        4,
        script(v0::ETH, [roll_hash.as_slice(), &eth(1)].concat())
            .packed()
            .unwrap(),
    );
    // Source is a protocol account, not the beneficiary's EOA.
    accounts.insert(
        5,
        script([88; 32], [roll_hash.as_slice(), &[9; 20]].concat())
            .packed()
            .unwrap(),
    );
    if large {
        for k in 2..=20u8 {
            accounts.insert(
                k as u32 + 4,
                script(v0::ETH, [roll_hash.as_slice(), &eth(k)].concat())
                    .packed()
                    .unwrap(),
            );
        }
        for i in 0..20u8 {
            accounts.insert(
                25 + i as u32,
                script([88; 32], [roll_hash.as_slice(), &[100 + i; 20]].concat())
                    .packed()
                    .unwrap(),
            );
        }
    }
    let mut native = Tree::default();
    for (id, sc) in &accounts {
        let sh = contract::script_hash(sc);
        native
            .update(
                build_account_field_key(*id, GW_ACCOUNT_SCRIPT_HASH_TYPE).into(),
                sh.into(),
            )
            .unwrap();
        let mut v = [0; 32];
        v[..4].copy_from_slice(&id.to_le_bytes());
        // Mainnet v0 stores LE32(id), with no v1 existence flag.
        native
            .update(build_script_hash_to_account_id_key(&sh).into(), v.into())
            .unwrap();
        native
            .update(short_key(sh[..20].try_into().unwrap()).into(), sh.into())
            .unwrap();
    }
    let d = hash(b"v0 signed settlement fixture deployment");
    let mut records: [BTreeMap<contract::Hash, Vec<u8>>; s::RECORD_KINDS] =
        std::array::from_fn(|_| BTreeMap::new());
    for (account, asset, baseline, eligible) in [
        (
            5,
            1,
            if large { 14000 * CAP } else { 1400 * CAP },
            if large { 6000 * CAP } else { 600 * CAP },
        ),
        (5, 3, 500, 100),
        (RAW_DESTINATION, 1, 80 * CAP, 0),
    ] {
        let sh = if account == RAW_DESTINATION {
            [0; 32]
        } else {
            contract::script_hash(&accounts[&account])
        };
        let address = if account == RAW_DESTINATION {
            [99; 20]
        } else {
            sh[..20].try_into().unwrap()
        };
        let src = Source {
            account,
            script_hash: sh,
            address,
            asset,
            type_hash: if asset == 1 { [0; 32] } else { th },
            baseline: baseline.into(),
            evidence: [10; 32],
        };
        let sk = source_key(d, &src);
        records[s::SOURCE_PLAN].insert(sk, encode(&src));
        native
            .update(
                NativeVersion::V0.balance_key(asset, address).into(),
                contract::value(baseline.into()).into(),
            )
            .unwrap();
        for (kind, amount) in [
            (INITIAL, eligible),
            (
                if account == RAW_DESTINATION {
                    PROTECTED
                } else {
                    EXCLUDED
                },
                baseline - eligible,
            ),
        ] {
            if amount == 0 {
                continue;
            }
            let lot = Lot {
                id: hash(&[sk.as_slice(), &[kind]].concat()),
                source: sk,
                kind,
                amount: amount.into(),
                evidence: [11; 32],
                reopen: [0; 32],
            };
            let lk = lot_key(d, &lot);
            records[s::LOT_PLAN].insert(lk, encode(&lot));
            if kind == INITIAL {
                let claim = Claim {
                    epoch: 0,
                    source: sk,
                    lot: lk,
                    recipient: 4,
                    script_hash: contract::script_hash(&accounts[&4]),
                    address: eth(1),
                    cap: amount.into(),
                    evidence: [12; 32],
                };
                records[s::CLAIM_PLAN].insert(claim_key(d, &claim), encode(&claim));
            }
        }
    }
    if large {
        // Benchmark fixture: split the token entitlement into 31 independent lots/claims.
        let token_claim_key = *records[s::CLAIM_PLAN]
            .iter()
            .find(|(_, b)| {
                let c: Claim = decode(b).unwrap();
                let src: Source = decode(&records[s::SOURCE_PLAN][&c.source]).unwrap();
                src.asset == 3
            })
            .unwrap()
            .0;
        let old: Claim = decode(&records[s::CLAIM_PLAN].remove(&token_claim_key).unwrap()).unwrap();
        let old_lot: Lot = decode(&records[s::LOT_PLAN].remove(&old.lot).unwrap()).unwrap();
        for i in 0..31u64 {
            let n = 100 * (i + 1) / 31 - 100 * i / 31;
            let mut lot = old_lot.clone();
            lot.id = hash(&[b"token".as_slice(), &i.to_le_bytes()].concat());
            lot.amount = n.into();
            let lk = lot_key(d, &lot);
            records[s::LOT_PLAN].insert(lk, encode(&lot));
            let mut c = old.clone();
            c.lot = lk;
            c.cap = n.into();
            let k = (i % 20 + 1) as u8;
            c.recipient = if k == 1 { 4 } else { k as u32 + 4 };
            c.script_hash = contract::script_hash(&accounts[&c.recipient]);
            c.address = eth(k);
            records[s::CLAIM_PLAN].insert(claim_key(d, &c), encode(&c));
        }
        let ck = *records[s::CLAIM_PLAN]
            .iter()
            .find(|(_, b)| {
                let c: Claim = decode(b).unwrap();
                let src: Source = decode(&records[s::SOURCE_PLAN][&c.source]).unwrap();
                src.asset == 1
            })
            .unwrap()
            .0;
        let old: Claim = decode(&records[s::CLAIM_PLAN].remove(&ck).unwrap()).unwrap();
        let old_lot: Lot = decode(&records[s::LOT_PLAN].remove(&old.lot).unwrap()).unwrap();
        for k in 1..=20u8 {
            let mut lot = old_lot.clone();
            lot.id = hash(&[b"ckb".as_slice(), &[k]].concat());
            lot.amount = (300 * CAP).into();
            let lk = lot_key(d, &lot);
            records[s::LOT_PLAN].insert(lk, encode(&lot));
            let mut c = old.clone();
            c.lot = lk;
            c.cap = lot.amount;
            c.recipient = if k == 1 { 4 } else { k as u32 + 4 };
            c.script_hash = contract::script_hash(&accounts[&c.recipient]);
            c.address = eth(k);
            records[s::CLAIM_PLAN].insert(claim_key(d, &c), encode(&c));
        }

        // Redistribute 51 claims across 20 source accounts (or one paged account), conserving balances.
        let claims: Vec<Claim> = records[s::CLAIM_PLAN]
            .values()
            .map(|b| decode(b).unwrap())
            .collect();
        records[s::CLAIM_PLAN].clear();
        let mut moved = BTreeMap::<contract::Hash, U256>::new();
        let mut new_sources = BTreeMap::<contract::Hash, Source>::new();
        for mut c in claims {
            let src: Source = decode(&records[s::SOURCE_PLAN][&c.source]).unwrap();
            *moved.entry(c.source).or_default() += c.cap;
            let index = if c.recipient == 4 { 0 } else { c.recipient - 5 };
            let id = if one_source { 25 } else { 25 + index };
            let sh = contract::script_hash(&accounts[&id]);
            let mut ns = src;
            ns.account = id;
            ns.script_hash = sh;
            ns.address = sh[..20].try_into().unwrap();
            ns.baseline = U256::zero();
            let sk = source_key(d, &ns);
            let entry = new_sources.entry(sk).or_insert(ns);
            entry.baseline += c.cap;
            let mut lot: Lot = decode(&records[s::LOT_PLAN].remove(&c.lot).unwrap()).unwrap();
            lot.source = sk;
            c.lot = lot_key(d, &lot);
            records[s::LOT_PLAN].insert(c.lot, encode(&lot));
            c.source = sk;
            records[s::CLAIM_PLAN].insert(claim_key(d, &c), encode(&c));
        }
        for (sk, n) in moved {
            let mut src: Source = decode(&records[s::SOURCE_PLAN][&sk]).unwrap();
            src.baseline -= n;
            native
                .update(
                    NativeVersion::V0.balance_key(src.asset, src.address).into(),
                    contract::value(src.baseline).into(),
                )
                .unwrap();
            records[s::SOURCE_PLAN].insert(sk, encode(&src));
        }
        for (sk, src) in new_sources {
            native
                .update(
                    NativeVersion::V0.balance_key(src.asset, src.address).into(),
                    contract::value(src.baseline).into(),
                )
                .unwrap();
            records[s::SOURCE_PLAN].insert(sk, encode(&src));
        }

        // 20,000 additional funded source accounts and claims, only to stress proof scale.
        for i in 0..20000u32 {
            let id = 45 + i;
            let sc = script([88; 32], [roll_hash.as_slice(), &id.to_le_bytes()].concat())
                .packed()
                .unwrap();
            let sh = contract::script_hash(&sc);
            let address = sh[..20].try_into().unwrap();
            accounts.insert(id, sc);
            native
                .update(
                    build_account_field_key(id, GW_ACCOUNT_SCRIPT_HASH_TYPE).into(),
                    sh.into(),
                )
                .unwrap();
            let mut idv = [0; 32];
            idv[..4].copy_from_slice(&id.to_le_bytes());
            native
                .update(build_script_hash_to_account_id_key(&sh).into(), idv.into())
                .unwrap();
            native.update(short_key(address).into(), sh.into()).unwrap();
            native
                .update(
                    NativeVersion::V0.balance_key(1, address).into(),
                    contract::value(U256::one()).into(),
                )
                .unwrap();
            let src = Source {
                account: id,
                script_hash: sh,
                address,
                asset: 1,
                type_hash: [0; 32],
                baseline: U256::one(),
                evidence: [10; 32],
            };
            let sk = source_key(d, &src);
            records[s::SOURCE_PLAN].insert(sk, encode(&src));
            let lot = Lot {
                id: hash(&[b"background".as_slice(), &id.to_le_bytes()].concat()),
                source: sk,
                kind: INITIAL,
                amount: U256::one(),
                evidence: [11; 32],
                reopen: [0; 32],
            };
            let lk = lot_key(d, &lot);
            records[s::LOT_PLAN].insert(lk, encode(&lot));
            let c = Claim {
                epoch: 0,
                source: sk,
                lot: lk,
                recipient: 4,
                script_hash: contract::script_hash(&accounts[&4]),
                address: eth(1),
                cap: U256::one(),
                evidence: [12; 32],
            };
            records[s::CLAIM_PLAN].insert(claim_key(d, &c), encode(&c));
        }
    }
    let mut control_scripts = BTreeMap::new();
    if std::env::var_os("GW_CONTROL_TEST").is_some() {
        let mut owners = vec![eth(1), eth(2), eth(4)];
        owners.sort();
        let sc = contract::control::descriptor(
            hash32(godwoken_withdraw::v0::ROLLUP).unwrap(),
            4,
            2,
            &owners,
        )
        .unwrap();
        control_scripts.insert(4, Script::from_packed(sc.clone()).unwrap());
        let claims = records[s::CLAIM_PLAN]
            .values()
            .map(|b| {
                let mut c: Claim = decode(b).unwrap();
                c.script_hash = contract::script_hash(&sc);
                c.address = contract::control::identity(&sc);
                (claim_key(d, &c), encode(&c))
            })
            .collect();
        records[s::CLAIM_PLAN] = claims;
    }
    let state = packed::GlobalState::from_slice(
        &bytes(snap["rollup_cell"]["output_data"].as_str().unwrap()).unwrap(),
    )
    .unwrap();
    let state = state
        .clone()
        .as_builder()
        .account(
            state
                .account()
                .as_builder()
                .merkle_root(packed::Byte32::from_slice(native.root().as_slice()).unwrap())
                .count((if large { 20045u32 } else { 6u32 }).pack())
                .build(),
        )
        .build();
    cells[0].output.type_script = Some(roll.clone());
    cells[0].output.capacity = format!("0x{:x}", 800 * CAP);
    cells[0].output_data = hx(state.as_slice());
    let mut outputs: Vec<_> = cells.iter().map(|c| c.output.clone()).collect();
    let mut data: Vec<_> = cells.iter().map(|c| c.output_data.clone()).collect();
    outputs.push(CellOutput {
        capacity: format!("0x{:x}", 1000 * CAP),
        lock: gov.clone(),
        type_script: None,
    });
    data.push("0x".into());
    outputs.push(CellOutput {
        capacity: format!("0x{:x}", if large { 20000 * CAP } else { 2000 * CAP }),
        lock: custody_cell.output.lock.clone(),
        type_script: None,
    });
    data.push("0x".into());
    outputs.push(CellOutput {
        capacity: format!("0x{:x}", 500 * CAP),
        lock: custody_cell.output.lock.clone(),
        type_script: Some(token.clone()),
    });
    data.push(hx(500u128.to_le_bytes()));
    let genesis = base_tx(outputs, data);
    let base = contract::Profile {
        rollup: roll.packed().unwrap(),
        config_hash: hash(&config),
        bootstrap_outpoint: withdraw::outpoint(&point(&genesis, 0)).unwrap(),
        bootstrap_capacity: 800 * CAP,
        bootstrap_lock: cells[0].output.lock.packed().unwrap(),
        bootstrap_state: state,
        governance: gov.packed().unwrap(),
        change: custody_cell.output.lock.packed().unwrap(),
        eoa_codes: vec![v0::ETH],
        catalogue: vec![(1, [0; 32]), (3, th)],
    };
    let auxiliary = (1..5)
        .map(|i| {
            let c = genesis.cell(&point(&genesis, i)).unwrap();
            let before = contract::Cell {
                capacity: quantity(&c.output.capacity).unwrap(),
                lock: c.output.lock.packed().unwrap(),
                type_: c.output.type_script.as_ref().map(|t| t.packed().unwrap()),
                data: bytes(&c.output_data).unwrap(),
            };
            let mut after = before.clone();
            if after.type_.is_some() {
                after.lock = base.governance.clone();
            }
            v0::Auxiliary {
                outpoint: withdraw::outpoint(&c.out_point).unwrap(),
                before,
                after,
            }
        })
        .collect();
    let mut profile = v0::Profile {
        settlement: s::SettlementProfile {
            base,
            deployment: d,
            manifest: [1; 32],
            state_root: hash32(
                &plan::initial_root(&[
                    records[s::SOURCE_PLAN].clone(),
                    records[s::LOT_PLAN].clone(),
                    records[s::CLAIM_PLAN].clone(),
                ])
                .unwrap(),
            )
            .unwrap(),
            source_count: records[s::SOURCE_PLAN].len() as u64,
            reserve_count: records[s::LOT_PLAN].len() as u64,
            initial_epoch: Epoch {
                id: 0,
                claim_count: if large { 20051 } else { 2 },
                manifest: [2; 32],
                policy: [3; 32],
                status: PENDING,
            },
            limits: vec![
                AssetLimit {
                    asset: 1,
                    type_hash: [0; 32],
                    ceiling: (if large { 10000 * CAP } else { 1000 * CAP }).into(),
                },
                AssetLimit {
                    asset: 3,
                    type_hash: th,
                    ceiling: 100.into(),
                },
            ],
        },
        auxiliary,
    };
    // Exercise bootstrap with the funded v0 catalogue size (19), including excluded budgets.
    for id in 1000..1017u32 {
        let h = hash(&id.to_le_bytes());
        profile.settlement.base.catalogue.push((id, h));
        profile.settlement.limits.push(AssetLimit {
            asset: id,
            type_hash: h,
            ceiling: 0.into(),
        });
    }
    let mut deps: Vec<_> = poa
        .cell_deps
        .iter()
        .map(|v| {
            let p: OutPoint = serde_json::from_value(v["out_point"].clone()).unwrap();
            Dependency {
                data_hash: hx(hash(&bytes(&node.live[&p].output_data).unwrap())),
                out_point: p,
                dep_type: "code".into(),
            }
        })
        .collect();
    let secp: Value =
        serde_json::from_str(include_str!("../fixtures/mainnet-secp-type.json")).unwrap();
    let typ: Script = serde_json::from_value(secp["type"].clone()).unwrap();
    assert_eq!(typ.hash().unwrap(), hx(v0::SECP));
    deploy(
        &mut node,
        &mut deps,
        Some(typ),
        ckb_system_scripts::BUNDLED_CELL
            .get("specs/cells/secp256k1_blake160_sighash_all")
            .unwrap()
            .to_vec(),
    );
    for name in ["custodian_lock", "state_validator"] {
        let e = super::v0_upgrade::evidence(&format!("{name}-recorded-live"));
        let c = &e["response"]["result"]["cell"];
        let typ = serde_json::from_value(c["output"]["type"].clone()).unwrap();
        let data = if name == "state_validator" {
            std::env::var("GW_V0_SETTLEMENT_BINARY")
                .map(|p| std::fs::read(p).unwrap())
                .unwrap_or_else(|_| vec![0])
        } else {
            bytes(c["data"]["content"].as_str().unwrap()).unwrap()
        };
        deploy(&mut node, &mut deps, Some(typ), data);
    }
    let encoded = v0::encode_profile(&profile);
    v0::decode_profile(&encoded, hash(&encoded)).unwrap();
    deploy(&mut node, &mut deps, None, encoded);
    deploy(&mut node, &mut deps, None, config.clone());
    let roll = point(&genesis, 0);
    let sponsor = point(&genesis, 5);
    let custody = vec![point(&genesis, 6), point(&genesis, 7)];
    node.insert(genesis);
    let ledger =
        Ledger::from_records(records, control_scripts, d, &profile.settlement.limits).unwrap();
    Fixture {
        node,
        deps,
        profile,
        config,
        native,
        ledger,
        accounts,
        roll,
        sponsor,
        custody,
        token,
    }
}
fn witness(lock: Option<Vec<u8>>, action: Option<Vec<u8>>) -> String {
    hx(packed::WitnessArgs::new_builder()
        .lock(lock.map(gw_types::bytes::Bytes::from).pack())
        .output_type(action.map(gw_types::bytes::Bytes::from).pack())
        .build()
        .as_slice())
}
fn sign_secp(tx: &mut Transaction, indices: &[usize], secret: u8) {
    let mut h = gw_hash::blake2b::new_blake2b();
    h.update(&hash32(&tx.hash().unwrap()).unwrap());
    for (j, i) in indices.iter().enumerate() {
        let mut w = bytes(&tx.witnesses[*i]).unwrap();
        if j == 0 {
            let a = packed::WitnessArgs::from_slice(&w).unwrap();
            w = a
                .as_builder()
                .lock(Some(gw_types::bytes::Bytes::from(vec![0; 65])).pack())
                .build()
                .as_slice()
                .to_vec();
        }
        h.update(&(w.len() as u64).to_le_bytes());
        h.update(&w);
    }
    for w in &tx.witnesses[tx.inputs.len()..] {
        let b = bytes(w).unwrap();
        h.update(&(b.len() as u64).to_le_bytes());
        h.update(&b);
    }
    let mut digest = [0; 32];
    h.finalize(&mut digest);
    let (id, sig) = Secp256k1::new()
        .sign_ecdsa_recoverable(&Message::from_digest(digest), &key(secret))
        .serialize_compact();
    let a = packed::WitnessArgs::from_slice(&bytes(&tx.witnesses[indices[0]]).unwrap()).unwrap();
    tx.witnesses[indices[0]] = hx(a
        .as_builder()
        .lock(
            Some(gw_types::bytes::Bytes::from(
                [sig.as_slice(), &[i32::from(id) as u8]].concat(),
            ))
            .pack(),
        )
        .build()
        .as_slice());
}
impl Fixture {
    fn action(
        &self,
        op: s::Operation,
        records: Vec<Vec<u8>>,
        native: contract::Action,
    ) -> Result<s::SettlementAction> {
        self.ledger.build_action_with(
            &self.profile.settlement,
            hash(&v0::encode_profile(&self.profile)),
            op,
            0,
            records,
            native,
            NativeVersion::V0,
            |keys| {
                let ks: Vec<_> = keys.iter().map(|k| (*k).into()).collect();
                Ok((
                    keys.iter()
                        .map(|k| Ok((*k, self.native.get(&(*k).into())?.as_slice().try_into()?)))
                        .collect::<Result<_>>()?,
                    self.native.merkle_proof(ks.clone())?.compile(ks)?,
                ))
            },
        )
    }
    fn transaction(&self, a: &s::SettlementAction) -> Transaction {
        let begin = a.op == s::Operation::Begin;
        let pay = a.op == s::Operation::Pay;
        let mut points = vec![self.roll.clone()];
        if begin {
            for x in &self.profile.auxiliary {
                points.push(OutPoint {
                    tx_hash: hx(x.outpoint.tx_hash().as_slice()),
                    index: format!(
                        "0x{:x}",
                        u32::from_le_bytes(x.outpoint.index().as_slice().try_into().unwrap())
                    ),
                });
            }
        }
        if pay {
            points.extend(self.custody.clone());
        }
        points.push(self.sponsor.clone());
        let e = v0::evaluate(&self.profile, hash(&v0::encode_profile(&self.profile)), a).unwrap();
        let mut leaves = a.native.kv.clone();
        for (k, v) in &mut leaves {
            if let Some(n) = e.native_writes.get(k) {
                *v = *n;
            }
        }
        let root = a
            .native
            .proof
            .compute_root::<gw_smt::smt::Blake2bHasher>(
                leaves
                    .iter()
                    .map(|(k, v)| ((*k).into(), (*v).into()))
                    .collect(),
            )
            .unwrap();
        let old = &self.node.live[&self.roll];
        let state = packed::GlobalState::from_slice(&bytes(&old.output_data).unwrap()).unwrap();
        let state = state
            .clone()
            .as_builder()
            .account(
                state
                    .account()
                    .as_builder()
                    .merkle_root(packed::Byte32::from_slice(root.as_slice()).unwrap())
                    .build(),
            )
            .build();
        let gov = Script::from_packed(self.profile.settlement.base.governance.clone()).unwrap();
        let mut outputs = vec![CellOutput {
            lock: gov.clone(),
            ..old.output.clone()
        }];
        let mut data = vec![hx(state.as_slice())];
        if begin {
            for x in &self.profile.auxiliary {
                outputs.push(CellOutput {
                    capacity: format!("0x{:x}", x.after.capacity),
                    lock: Script::from_packed(x.after.lock.clone()).unwrap(),
                    type_script: x
                        .after
                        .type_
                        .clone()
                        .map(|s| Script::from_packed(s).unwrap()),
                });
                data.push(hx(&x.after.data));
            }
        }
        if pay {
            let capacity = e.payments[&(0, 1)].as_u64();
            let amount = e.payments[&(0, 3)].as_u128();
            outputs.push(CellOutput {
                capacity: format!("0x{capacity:x}"),
                lock: claim::owner(&hx(eth(1))).unwrap(),
                type_script: Some(self.token.clone()),
            });
            data.push(hx(amount.to_le_bytes()));
            let plain = &self.node.live[&self.custody[0]];
            outputs.push(CellOutput {
                capacity: format!(
                    "0x{:x}",
                    quantity(&plain.output.capacity).unwrap() - capacity
                ),
                ..plain.output.clone()
            });
            data.push("0x".into());
            let token = &self.node.live[&self.custody[1]];
            outputs.push(token.output.clone());
            data.push(hx((u128::from_le_bytes(
                bytes(&token.output_data).unwrap().try_into().unwrap(),
            ) - amount)
                .to_le_bytes()));
        }
        let sponsor = &self.node.live[&self.sponsor];
        outputs.push(CellOutput {
            capacity: format!(
                "0x{:x}",
                quantity(&sponsor.output.capacity).unwrap() - 100_000
            ),
            ..sponsor.output.clone()
        });
        data.push("0x".into());
        let mut tx = base_tx(outputs, data);
        tx.inputs = points
            .iter()
            .map(|p| json!({"previous_output":p,"since":"0x0"}))
            .collect();
        tx.cell_deps = self
            .deps
            .iter()
            .map(|d| json!({"out_point":d.out_point,"dep_type":d.dep_type}))
            .collect();
        tx.witnesses = vec!["0x".into(); points.len()];
        tx.witnesses[0] = witness(
            if begin { None } else { Some(vec![0; 65]) },
            Some(v0::encode_action(a)),
        );
        godwoken_withdraw::settlement::replay::frame_transaction(&mut tx).unwrap();
        if begin {
            tx.witnesses[2] = witness(Some(omni_signature(&[0; 65])), None);
            tx.witnesses[3] = witness(Some(omni_signature(&[0; 65])), None);
            let n = points.len() - 1;
            tx.witnesses[n] = witness(Some(vec![0; 65]), None);
            sign_group(&mut tx, &[2], 1, true);
            sign_group(&mut tx, &[3], 2, true);
            sign_secp(&mut tx, &[n], 3);
        } else {
            sign_secp(&mut tx, &[0, points.len() - 1], 3);
        }
        tx
    }
    fn apply(&mut self, a: &s::SettlementAction, tx: Transaction, vm: bool) {
        let cells: Vec<_> = tx
            .input_points()
            .unwrap()
            .iter()
            .map(|p| self.node.live[p].clone())
            .collect();
        let resolved = withdraw::chain::resolved(&tx, &cells).unwrap();
        let out = v0::verify(
            &self.profile,
            hash(&v0::encode_profile(&self.profile)),
            &self.config,
            &resolved,
            a,
        )
        .unwrap();
        if vm {
            let cycles = self.node.vm(&tx).unwrap();
            println!("V0 {:?}: {cycles} cycles", a.op);
            assert!(cycles < 70_000_000);
        }
        let e = v0::evaluate(&self.profile, hash(&v0::encode_profile(&self.profile)), a).unwrap();
        for (k, v) in &e.native_writes {
            self.native.update((*k).into(), (*v).into()).unwrap();
        }
        assert_eq!(self.native.root().as_slice(), out.root);
        self.ledger.apply(e).unwrap();
        self.roll = point(&tx, 0);
        let gov = Script::from_packed(self.profile.settlement.base.governance.clone()).unwrap();
        self.sponsor = point(
            &tx,
            tx.outputs
                .iter()
                .position(|o| o.lock == gov && o.type_script.is_none())
                .unwrap(),
        );
        if a.op == s::Operation::Pay {
            let consumed = tx.input_points().unwrap();
            self.custody.retain(|p| !consumed.contains(p));
            for (i, c) in tx.outputs.iter().enumerate() {
                if c.lock.code_hash
                    == hx(v0::config(&self.config)
                        .unwrap()
                        .custodian_script_type_hash()
                        .as_slice())
                {
                    self.custody.push(point(&tx, i));
                }
            }
            self.custody.sort_by_key(|p| {
                if p.tx_hash == tx.hash().unwrap() {
                    tx.outputs[quantity(&p.index).unwrap() as usize]
                        .type_script
                        .is_some()
                } else {
                    self.node.live[p].output.type_script.is_some()
                }
            });
        }
        self.node.insert(tx);
    }
    fn active(&mut self, vm: bool) {
        for _ in 0..30 {
            if self.ledger.metadata.is_some() {
                assert!(self
                    .ledger
                    .next_with_profile_hash(hash(&v0::encode_profile(&self.profile)), 2)
                    .unwrap()
                    .is_none());
                return;
            }
            let (op, records) = self
                .ledger
                .next(&self.profile.settlement, 2)
                .unwrap()
                .unwrap();
            let a = self.action(op, records, empty_action()).unwrap();
            let tx = self.transaction(&a);
            self.apply(&a, tx, vm);
        }
        panic!("not active");
    }
    fn payment(&self, ckb: u64, token: u64) -> Result<s::SettlementAction> {
        let records = self.ledger.records[s::CLAIM_PLAN]
            .values()
            .map(|r| {
                let c: Claim = decode(r).unwrap();
                let src = self.ledger.source(c.source).unwrap();
                encode(&Payment {
                    claim: c,
                    amount: if src.asset == 1 {
                        ckb.into()
                    } else {
                        token.into()
                    },
                })
            })
            .collect();
        let mut a = empty_action();
        a.recipients.push(contract::Recipient {
            address: self
                .ledger
                .recipient_scripts
                .get(&4)
                .map(|s| contract::control::identity(&s.packed().unwrap()))
                .unwrap_or(eth(1)),
            id: 4,
            script: self
                .ledger
                .recipient_scripts
                .get(&4)
                .map(|s| s.packed().unwrap())
                .unwrap_or_else(|| self.accounts[&4].clone()),
        });
        for id in [1, 3] {
            a.assets.push(contract::Asset {
                id,
                script: self.accounts[&id].clone(),
            });
        }
        a.payouts.push(contract::Payout {
            output: 1,
            recipient: 0,
            asset: 3,
        });
        self.action(s::Operation::Pay, records, a)
    }
}
#[test]
fn export_v0_profile() {
    let f = fixture();
    if let Ok(path) = std::env::var("GW_V0_SETTLEMENT_PROFILE") {
        let b = v0::encode_profile(&f.profile);
        std::fs::write(&path, &b).unwrap();
        std::fs::write(format!("{path}.hash"), hex::encode(hash(&b))).unwrap();
    }
}
#[test]
fn v0_shared_settlement_full_signed_lifecycle() {
    let vm = std::env::var_os("GW_V0_SETTLEMENT_BINARY").is_some();
    let mut f = fixture();
    f.active(vm);
    for _ in 0..2 {
        let a = f.payment(300 * CAP, 50).unwrap();
        let tx = f.transaction(&a);
        f.apply(&a, tx.clone(), vm);
        let cells = vec![f.node.live[&point(&tx, 1)].clone()];
        let (_, _, _, assets, mut request) = setup_claim(); // catalogue below only needs L1 identity.
        request.inputs = vec![point(&tx, 1)];
        let token_asset = assets
            .iter()
            .find(|x| x.type_hash == f.token.hash().unwrap())
            .cloned();
        assert!(
            token_asset.is_some(),
            "fixture token missing in L1 catalogue"
        );
        let q = claim::construct(&request, cells, &assets, &f.deps, 1000).unwrap();
        let user = claim::assemble(&q, &signature(&q.signing_message, 1)).unwrap();
        if vm {
            let n = f.node.vm(&user).unwrap();
            println!("V0 user OmniLock exit: {n} cycles");
            assert!(n < 70_000_000);
        }
        f.node.insert(user.clone());
        assert!(f.node.vm(&user).is_err());
    }
    assert!(f.payment(1, 1).is_err());
    let sh = contract::script_hash(&f.accounts[&5]);
    let address = sh[..20].try_into().unwrap();
    assert_eq!(
        contract::amount(
            f.native
                .get(&NativeVersion::V0.balance_key(1, address).into())
                .unwrap()
                .into()
        ),
        (800 * CAP).into()
    );
    assert_eq!(
        contract::amount(
            f.native
                .get(&NativeVersion::V0.balance_key(3, address).into())
                .unwrap()
                .into()
        ),
        400.into()
    );
}

#[test]
fn v0_governance_capacity_top_up_keeps_native_debit_unchanged() {
    let mut fixture = fixture();
    fixture.active(false);
    let action = fixture.payment(300 * CAP, 50).unwrap();
    let mut tx = fixture.transaction(&action);
    let verify = |tx: &Transaction| {
        let cells = tx
            .input_points()
            .unwrap()
            .iter()
            .map(|point| fixture.node.live[point].clone())
            .collect::<Vec<_>>();
        let resolved = withdraw::chain::resolved(tx, &cells).unwrap();
        v0::verify(
            &fixture.profile,
            hash(&v0::encode_profile(&fixture.profile)),
            &fixture.config,
            &resolved,
            &action,
        )
    };
    let baseline = verify(&tx).unwrap();
    let extra = 10 * CAP;
    tx.outputs[1].capacity = format!("0x{:x}", quantity(&tx.outputs[1].capacity).unwrap() + extra);
    assert!(std::panic::catch_unwind(|| verify(&tx)).is_err());
    if std::env::var_os("GW_V0_SETTLEMENT_BINARY").is_some() {
        resign(&fixture, &mut tx);
        let error = fixture.node.vm(&tx).unwrap_err().to_string();
        assert!(
            error.contains("Type") && error.contains("ValidationFailure"),
            "{error}"
        );
    }
    let sponsor = tx.outputs.last_mut().unwrap();
    sponsor.capacity = format!("0x{:x}", quantity(&sponsor.capacity).unwrap() - extra);
    let funded = verify(&tx).unwrap();
    assert_eq!(funded.debits, baseline.debits);
    assert_eq!(funded.fee, baseline.fee);
    assert_eq!(funded.top_up_shannons, extra);
    if std::env::var_os("GW_V0_SETTLEMENT_BINARY").is_some() {
        resign(&fixture, &mut tx);
        let cycles = fixture.node.vm(&tx).unwrap();
        println!("V0 governance-funded payout: {cycles} cycles");
    }
}

fn replace_action(tx: &mut Transaction, a: &s::SettlementAction) {
    tx.witnesses.truncate(tx.inputs.len());
    let w = packed::WitnessArgs::from_slice(&bytes(&tx.witnesses[0]).unwrap()).unwrap();
    tx.witnesses[0] = hx(w
        .as_builder()
        .output_type(Some(gw_types::bytes::Bytes::from(v0::encode_action(a))).pack())
        .build()
        .as_slice());
    godwoken_withdraw::settlement::replay::frame_transaction(tx).unwrap();
}
fn decode_transaction(tx: &Transaction) -> s::SettlementAction {
    let w = packed::WitnessArgs::from_slice(&bytes(&tx.witnesses[0]).unwrap()).unwrap();
    let data = w.output_type().to_opt().unwrap().raw_data();
    v0::decode_action(&godwoken_withdraw::settlement::replay::unframe(tx, &data).unwrap()).unwrap()
}
fn resign(f: &Fixture, tx: &mut Transaction) {
    let gov = Script::from_packed(f.profile.settlement.base.governance.clone()).unwrap();
    let indices: Vec<_> = tx
        .input_points()
        .unwrap()
        .iter()
        .enumerate()
        .filter(|(_, p)| f.node.live[*p].output.lock == gov)
        .map(|(i, _)| i)
        .collect();
    sign_secp(tx, &indices, 3);
}
fn rejected(f: &Fixture, name: &str, mut tx: Transaction, a: s::SettlementAction) {
    let cells: Vec<_> = tx
        .input_points()
        .unwrap()
        .iter()
        .map(|p| f.node.live[p].clone())
        .collect();
    let resolved = withdraw::chain::resolved(&tx, &cells).unwrap();
    let result = std::panic::catch_unwind(|| {
        v0::verify(
            &f.profile,
            hash(&v0::encode_profile(&f.profile)),
            &f.config,
            &resolved,
            &a,
        )
    });
    assert!(!matches!(result, Ok(Ok(_))), "host accepted {name}");
    if std::env::var_os("GW_V0_SETTLEMENT_BINARY").is_some() {
        replace_action(&mut tx, &a);
        resign(f, &mut tx);
        let err = f.node.vm(&tx).expect_err(name).to_string();
        assert!(
            err.contains("Type") && err.contains("ValidationFailure"),
            "{name}: wrong rejection: {err}"
        );
        println!("V0 contract rejected {name}: {err}");
    }
}
#[test]
fn v0_signed_adversarial_and_second_overdraw() {
    let mut f = fixture();
    f.active(false);
    let a = f.payment(300 * CAP, 50).unwrap();
    let tx = f.transaction(&a);
    for kind in [
        s::SOURCE_PLAN,
        s::LOT_PLAN,
        s::CLAIM_PLAN,
        s::SOURCES,
        s::RESERVES,
        s::PAID,
    ] {
        let mut bad = a.clone();
        super::settlement_e2e::tamper_state_record(&mut bad, kind, true);
        rejected(
            &f,
            &format!("missing root-only tree {kind}"),
            tx.clone(),
            bad,
        );
        let mut bad = a.clone();
        super::settlement_e2e::tamper_state_record(&mut bad, kind, false);
        rejected(
            &f,
            &format!("forged root-only leaf {kind}"),
            tx.clone(),
            bad,
        );
    }

    let mut bad = a.clone();
    bad.records.push(bad.records[0].clone());
    rejected(&f, "duplicate payment", tx.clone(), bad);
    let mut bad = a.clone();
    bad.native.recipients.push(bad.native.recipients[0].clone());
    rejected(&f, "duplicate ETH address", tx.clone(), bad);
    let mut bad = a.clone();
    bad.native.recipients[0].address = contract::script_hash(&f.accounts[&4])[..20]
        .try_into()
        .unwrap();
    rejected(&f, "short address instead of ETH owner", tx.clone(), bad);
    let mut bad = a.clone();
    bad.native.recipients[0].script = script(
        v0::ETH,
        [
            contract::script_hash(&f.profile.settlement.base.rollup).as_slice(),
            &eth(2),
        ]
        .concat(),
    )
    .packed()
    .unwrap();
    rejected(&f, "wrong EOA script", tx.clone(), bad);
    let mut bad = a.clone();
    bad.native.kv[0].1[0] ^= 1;
    rejected(&f, "wrong old leaf", tx.clone(), bad);
    let mut bad = a.clone();
    bad.native.proof.0[0] ^= 1;
    rejected(&f, "corrupted native proof", tx.clone(), bad);
    let mut bad = a.clone();
    bad.trees[0].proof.0[0] ^= 1;
    rejected(&f, "corrupted allocation proof", tx.clone(), bad);
    let mut bad = a.clone();
    bad.after[0] ^= 1;
    rejected(&f, "forged metadata", tx.clone(), bad);
    let mut bad = tx.clone();
    bad.outputs[1].lock = claim::owner(&hx(eth(2))).unwrap();
    rejected(&f, "wrong output ETH", bad, a.clone());
    let mut bad = tx.clone();
    bad.outputs[1].lock.args.push_str("00");
    rejected(&f, "extra OmniLock args", bad, a.clone());
    let mut bad = tx.clone();
    bad.outputs[1].capacity = "0x1".into();
    rejected(&f, "insufficient output CKB", bad, a.clone());
    let mut bad = tx.clone();
    bad.outputs_data[1] = hx(51u128.to_le_bytes());
    rejected(&f, "wrong sUDT amount", bad, a.clone());
    let mut bad = tx.clone();
    bad.outputs[1].type_script.as_mut().unwrap().args = hx([22; 32]);
    rejected(&f, "wrong sUDT identity", bad, a.clone());
    let mut bad = tx.clone();
    bad.outputs.remove(1);
    bad.outputs_data.remove(1);
    rejected(&f, "missing recipient cell", bad, a.clone());
    let mut bad = tx.clone();
    bad.outputs.push(bad.outputs[1].clone());
    bad.outputs_data.push(bad.outputs_data[1].clone());
    rejected(&f, "extra recipient output", bad, a.clone());
    let mut bad = tx.clone();
    bad.outputs.push(bad.outputs[2].clone());
    bad.outputs_data.push(bad.outputs_data[2].clone());
    rejected(&f, "extra custodian output", bad, a.clone());
    let mut bad = tx.clone();
    let mut args = bytes(&bad.outputs[2].lock.args).unwrap();
    let n = args.len();
    args[n - 8..].copy_from_slice(&u64::MAX.to_le_bytes());
    bad.outputs[2].lock.args = hx(args);
    rejected(&f, "unfinalized custodian change", bad, a.clone());
    let mut bad = tx.clone();
    let mut args = bytes(&bad.outputs[2].lock.args).unwrap();
    let n = args.len();
    args[n - 9] ^= 1;
    bad.outputs[2].lock.args = hx(args);
    rejected(&f, "unrecognized custodian change lock", bad, a.clone());
    let mut bad = tx.clone();
    let mut state = bytes(&bad.outputs_data[0]).unwrap();
    state[32] ^= 1;
    bad.outputs_data[0] = hx(state);
    rejected(&f, "wrong new SMT root", bad, a.clone());
    let mut bad = tx.clone();
    let mut state = bytes(&bad.outputs_data[0]).unwrap();
    let n = state.len();
    state[n - 2] ^= 1;
    bad.outputs_data[0] = hx(state);
    rejected(&f, "unrelated GlobalState mutation", bad, a.clone());
    let sh = contract::script_hash(&f.accounts[&5]);
    let bk = NativeVersion::V0.balance_key(1, sh[..20].try_into().unwrap());
    let mut bad = a.clone();
    bad.native.kv.iter_mut().find(|(k, _)| *k == bk).unwrap().1[16] = 1;
    rejected(&f, "noncanonical u128", tx.clone(), bad);
    let mut bad = a.clone();
    bad.native.kv.iter_mut().find(|(k, _)| *k == bk).unwrap().1 = contract::value(1.into());
    rejected(&f, "insufficient native source", tx.clone(), bad);
    let mut bad = a.clone();
    bad.before.clear();
    bad.op = s::Operation::Begin;
    rejected(&f, "repeat bootstrap", tx.clone(), bad);
    assert!(s::codec::decode_action(&v0::encode_action(&a)).is_err());
    assert!(v0::decode_action(&s::codec::encode_action(&a)).is_err());
    assert!(v0::decode_profile(
        &s::codec::encode_profile(&f.profile.settlement),
        hash(&s::codec::encode_profile(&f.profile.settlement))
    )
    .is_err());
    f.apply(
        &a,
        tx.clone(),
        std::env::var_os("GW_V0_SETTLEMENT_BINARY").is_some(),
    );
    assert!(f.node.vm(&tx).is_err());
    let second = f.payment(300 * CAP, 50).unwrap();
    let second_tx = f.transaction(&second);
    for (name, extra) in [
        ("second payment over remainder", 1u64),
        ("v1 CKB scale applied to v0", 10_000_000_000u64),
    ] {
        let mut bad = second.clone();
        let mut p: Payment = decode(&bad.records[0]).unwrap();
        p.amount += U256::from(extra);
        bad.records[0] = encode(&p);
        rejected(&f, name, second_tx.clone(), bad);
    }
    let mut stale = second.clone();
    stale.before = a.before.clone();
    rejected(&f, "stale paid ledger", second_tx.clone(), stale);
    f.apply(
        &second,
        second_tx,
        std::env::var_os("GW_V0_SETTLEMENT_BINARY").is_some(),
    );
    assert!(f.payment(1, 1).is_err());
}
#[test]
fn v0_bootstrap_is_two_producer_then_single_authority() {
    let vm = std::env::var_os("GW_V0_SETTLEMENT_BINARY").is_some();
    let mut f = fixture();
    let a = f
        .action(s::Operation::Begin, vec![], empty_action())
        .unwrap();
    let tx = f.transaction(&a);
    if vm {
        for i in [2, 3, 5] {
            let mut bad = tx.clone();
            bad.witnesses[i] = witness(None, None);
            let err = f.node.vm(&bad).unwrap_err().to_string();
            assert!(err.contains("Lock"), "{err}");
        }
        for (name, i) in [
            ("helper changed", 1),
            ("producer refunded to another lock", 2),
        ] {
            let mut bad = tx.clone();
            bad.outputs[i].lock = claim::owner(&hx(eth(8))).unwrap();
            sign_group(&mut bad, &[2], 1, true);
            sign_group(&mut bad, &[3], 2, true);
            sign_secp(&mut bad, &[5], 3);
            let err = f.node.vm(&bad).expect_err(name).to_string();
            assert!(err.contains("Type"), "{err}");
        }
        let mut bad = tx.clone();
        bad.cell_deps
            .push(json!({"out_point":tx.input_points().unwrap()[1],"dep_type":"code"}));
        sign_group(&mut bad, &[2], 1, true);
        sign_group(&mut bad, &[3], 2, true);
        sign_secp(&mut bad, &[5], 3);
        assert!(
            f.node.vm(&bad).is_err(),
            "setup as dep must not select normal PoA path"
        );
    }
    f.apply(&a, tx, vm);
    let a = f.payment(300 * CAP, 50).unwrap();
    let mut tx = f.transaction(&a);
    let governance = tx
        .input_points()
        .unwrap()
        .iter()
        .enumerate()
        .filter_map(|(i, p)| {
            (f.node.live[p].output.lock.packed().unwrap() == f.profile.settlement.base.governance)
                .then_some(i)
        })
        .collect::<Vec<_>>();
    assert_eq!(governance.len(), 2);
    if vm {
        sign_secp(&mut tx, &governance, 9);
        assert!(f.node.vm(&tx).unwrap_err().to_string().contains("Lock"));
        sign_secp(&mut tx, &governance, 3);
        for tag in [0u32, 1, 2, 3, 4, 5] {
            let mut bad = tx.clone();
            let mut b = v0::encode_action(&a);
            b[..4].copy_from_slice(&tag.to_le_bytes());
            let w = packed::WitnessArgs::from_slice(&bytes(&bad.witnesses[0]).unwrap()).unwrap();
            bad.witnesses[0] = hx(w
                .as_builder()
                .output_type(Some(gw_types::bytes::Bytes::from(b)).pack())
                .build()
                .as_slice());
            resign(&f, &mut bad);
            assert!(f.node.vm(&bad).unwrap_err().to_string().contains("Type"));
        }
    }
    f.apply(&a, tx, vm);
}
#[test]
fn v0_paid_cells_partial_user_exit_and_signed_overdraw() {
    let vm = std::env::var_os("GW_V0_SETTLEMENT_BINARY").is_some();
    let mut f = fixture();
    f.active(false);
    let a = f.payment(600 * CAP, 100).unwrap();
    let tx = f.transaction(&a);
    f.apply(&a, tx.clone(), vm);
    let (_, _, _, assets, mut req) = setup_claim();
    let id = assets
        .iter()
        .find(|x| x.type_hash == f.token.hash().unwrap())
        .unwrap()
        .asset_id;
    req.inputs = vec![point(&tx, 1)];
    req.amounts = Some(vec![
        claim::ClaimAmount {
            asset_id: 1,
            raw: (300 * CAP).to_string(),
        },
        claim::ClaimAmount {
            asset_id: id,
            raw: "40".into(),
        },
    ]);
    let q = claim::construct(
        &req,
        vec![f.node.live[&point(&tx, 1)].clone()],
        &assets,
        &f.deps,
        1000,
    )
    .unwrap();
    let first = claim::assemble(&q, &signature(&q.signing_message, 1)).unwrap();
    let owner = claim::owner(&req.address).unwrap();
    let remaining: Vec<_> = first
        .outputs
        .iter()
        .enumerate()
        .filter(|(_, c)| c.lock == owner)
        .map(|(i, _)| first.cell(&point(&first, i)).unwrap())
        .collect();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].output_data, hx(60u128.to_le_bytes()));
    if vm {
        println!(
            "V0 partial user exit: {} cycles",
            f.node.vm(&first).unwrap()
        );
    }
    f.node.insert(first.clone());
    req.inputs = remaining.iter().map(|c| c.out_point.clone()).collect();
    req.amounts = None;
    let q = claim::construct(&req, remaining, &assets, &f.deps, 1000).unwrap();
    if vm {
        let mut forged = q.clone();
        forged.transaction.outputs_data[0] = hx(61u128.to_le_bytes());
        forged.signing_message = hx(claim::signing_message(&forged.transaction).unwrap());
        forged.transaction_hash = forged.transaction.hash().unwrap();
        let bad = claim::assemble(&forged, &signature(&forged.signing_message, 1)).unwrap();
        let err = f.node.vm(&bad).unwrap_err().to_string();
        assert!(
            err.contains("Type") && err.contains("error code -52"),
            "{err}"
        );
        let wrong = claim::assemble(&q, &signature(&q.signing_message, 2));
        assert!(wrong.is_err());
    }
    let last = claim::assemble(&q, &signature(&q.signing_message, 1)).unwrap();
    for (o, d) in last.outputs.iter().zip(&last.outputs_data) {
        assert_eq!(o.lock, claim::parse_address(&req.destination).unwrap());
        assert!(quantity(&o.capacity).unwrap() >= claim::occupied(o, d).unwrap());
    }
    if vm {
        println!(
            "V0 remaining user exit: {} cycles",
            f.node.vm(&last).unwrap()
        );
    }
    let root = *f.native.root();
    f.node.insert(last.clone());
    assert!(f.node.vm(&last).is_err());
    assert_eq!(*f.native.root(), root);
}

#[test]
fn v0_native_keys_profile_and_protected_reserves() {
    for id in [1, 19, 172, u32::MAX] {
        for address in [[0; 20], [1; 20], [255; 20]] {
            assert_eq!(
                NativeVersion::V0.balance_key(id, address),
                godwoken_withdraw::v0::balance_key(id, &address)
            );
            assert_ne!(
                NativeVersion::V0.balance_key(id, address),
                NativeVersion::V1.balance_key(id, address)
            );
        }
    }
    assert_eq!(NativeVersion::V0.scale(), 1);
    assert!(NativeVersion::V0.supply_key(1).is_none());
    assert_eq!(
        NativeVersion::V0
            .amount(contract::value(u128::MAX.into()))
            .unwrap(),
        U256::from(u128::MAX)
    );
    assert!(NativeVersion::V0
        .amount(contract::value(U256::from(u128::MAX) + 1))
        .is_err());
    let mut f = fixture();
    let encoded = v0::encode_profile(&f.profile);
    let mut trailing = encoded.clone();
    trailing.push(0);
    assert!(v0::decode_profile(&trailing, hash(&trailing)).is_err());
    let mut bad = f.profile.clone();
    bad.settlement.base.governance = contract::eth_lock(eth(3));
    let b = v0::encode_profile(&bad);
    assert!(v0::decode_profile(&b, hash(&b)).is_err());
    let raw_src: Source = f.ledger.records[s::SOURCE_PLAN]
        .values()
        .map(|r| decode(r).unwrap())
        .find(|s: &Source| s.account == RAW_DESTINATION)
        .unwrap();
    let k = source_key(f.profile.settlement.deployment, &raw_src);
    assert!(NativeVersion::V0.protected(&raw_src));
    let a = f
        .action(s::Operation::Begin, vec![], empty_action())
        .unwrap();
    let tx = f.transaction(&a);
    f.apply(&a, tx, false);
    assert!(f
        .ledger
        .next_with_profile_hash(hash(&encoded), 2)
        .unwrap()
        .is_none());
    assert!(f.ledger.records[s::SOURCES].is_empty());
    // Root publication does not turn protected raw destinations into payable claims.
    assert!(f.ledger.records[s::CLAIM_PLAN]
        .values()
        .all(|r| decode::<Claim>(r).unwrap().source != k));
}

#[test]
fn v0_refund_only_before_bootstrap_and_after_activation() {
    use crate::refund_shutdown::*;
    assert!(
        std::env::var_os("GW_V0_SETTLEMENT_BINARY").is_some(),
        "Run test-v0-settlement-e2e.sh"
    );
    let mut f = fixture();
    for before_bootstrap in [true, false] {
        if !before_bootstrap {
            f.active(false);
        }
        let a = if before_bootstrap {
            f.action(s::Operation::Begin, vec![], empty_action())
                .unwrap()
        } else {
            f.payment(300 * CAP, 50).unwrap()
        };
        let original = f.transaction(&a);
        assert!(f.node.vm(&original).is_ok());
        let sign = |tx: &mut Transaction| {
            if before_bootstrap {
                sign_group(tx, &[2], 1, true);
                sign_group(tx, &[3], 2, true);
            }
            resign(&f, tx);
        };
        for (name, payload) in forbidden_actions(&v0::encode_action(&a)) {
            let mut bad = original.clone();
            set_payload(&mut bad, &payload);
            sign(&mut bad);
            assert_type_rejection(&f.node, &bad, name, Some(contract::Error::Encoding as i8));
        }
        for (name, mut bad) in frozen_state_attacks(&original) {
            sign(&mut bad);
            assert_type_rejection(&f.node, &bad, name, None);
        }
        assert!(f.node.vm(&original).is_ok());
    }
}

impl godwoken_withdraw::settlement::adapter::NativeDb for Fixture {
    fn protocol(&self) -> godwoken_withdraw::settlement::adapter::Protocol {
        godwoken_withdraw::settlement::adapter::Protocol::V0
    }
    fn root(&self) -> String {
        hx(self.native.root().as_slice())
    }
    fn account(&self, id: u32) -> Result<(contract::Hash, Script)> {
        let sc = self.accounts.get(&id).context("Fixture account")?;
        Ok((contract::script_hash(sc), Script::from_packed(sc.clone())?))
    }
    fn script(&self, _: &contract::Hash, _: bool) -> Result<Option<Script>> {
        Ok(None)
    }
    fn lookup_eth(&self, address: &[u8; 20]) -> Result<Value> {
        let (id, sc) = self
            .accounts
            .iter()
            .find(|(_, s)| {
                s.code_hash().as_slice() == v0::ETH && s.args().raw_data()[32..] == *address
            })
            .context("Fixture owner")?;
        Ok(json!({"status":"registered","account_id":id,"script":Script::from_packed(sc.clone())?}))
    }
    fn overlay(&self) -> Result<godwoken_withdraw::withdraw::state::RecoveryState<'_>> {
        Ok(
            godwoken_withdraw::withdraw::state::RecoveryState::from_store(
                *self.native.root(),
                self.native.store(),
            ),
        )
    }
    fn anchor(&self, _: &packed::GlobalState) -> Result<()> {
        Ok(())
    }
}
impl Fixture {
    fn deployment(&self) -> Deployment {
        Deployment {
            profile: hx(v0::encode_profile(&self.profile)),
            profile_hash: hx(hash(&v0::encode_profile(&self.profile))),
            validator_data_hash: self
                .deps
                .iter()
                .find(|d| {
                    self.node.live[&d.out_point]
                        .output
                        .type_script
                        .as_ref()
                        .and_then(|s| s.hash().ok())
                        == Some(hx(self
                            .profile
                            .settlement
                            .base
                            .rollup
                            .code_hash()
                            .as_slice()))
                })
                .unwrap()
                .data_hash
                .clone(),
            cell_deps: self.deps.clone(),
            sponsor_inputs: vec![self.sponsor.clone()],
            multisig_config: "0x".into(),
            bootstrap_lock_placeholder: None,
            fee_shannons: 100_000,
            confirmations: 1,
        }
    }
    fn production(
        &self,
        op: s::Operation,
        records: Vec<Vec<u8>>,
        amounts: Option<BTreeMap<u32, String>>,
    ) -> Skeleton {
        let requests = amounts.map(|amounts| {
            vec![Request::Amounts {
                eth_address: hx(self
                    .ledger
                    .recipient_scripts
                    .get(&4)
                    .map(|s| contract::control::identity(&s.packed().unwrap()))
                    .unwrap_or(eth(1))),
                amounts,
            }]
        });
        self.production_requests(op, records, requests)
    }
    fn production_requests(
        &self,
        op: s::Operation,
        records: Vec<Vec<u8>>,
        requests: Option<Vec<Request>>,
    ) -> Skeleton {
        use godwoken_withdraw::settlement::{self as tool, adapter::NativeDb};
        let d = self.deployment();
        let b = godwoken_withdraw::model::Bundle {
            schema_version: 1,
            network: "mainnet_v0".into(),
            observed_start: String::new(),
            observed_end: String::new(),
            rollup_cell: self.node.live[&self.roll].clone(),
            config_data: hx(&self.config),
            cells: godwoken_withdraw::model::FAMILIES
                .iter()
                .map(|f| {
                    (
                        f.to_string(),
                        if *f == "custodian" {
                            self.custody
                                .iter()
                                .map(|p| self.node.live[p].clone())
                                .collect()
                        } else {
                            vec![]
                        },
                    )
                })
                .collect(),
            consistency: String::new(),
            live_cells_checked: 0,
            ckb_rpc: None,
            indexer_rpc: None,
            indexer_before: Value::Null,
            indexer_after: Value::Null,
        };
        let deps = self
            .deps
            .iter()
            .map(|d| self.node.live[&d.out_point].clone())
            .collect();
        let native = self.overlay().unwrap();
        let sk = if let Some(requests) = requests {
            tool::build::construct_with_controls(
                self,
                &self.ledger,
                records,
                0,
                &native,
                &d,
                &b,
                &requests,
                vec![self.node.live[&self.sponsor].clone()],
                deps,
                json!([]),
                &if self.ledger.recipient_scripts.is_empty() {
                    vec![]
                } else {
                    vec![tool::build::ControlSpend {
                        account_id: 4,
                        destination: claim::owner(&hx(eth(1))).unwrap(),
                        authorization_cells: (1..=2)
                            .map(|k| {
                                self.node
                                    .live
                                    .values()
                                    .find(|c| {
                                        c.output.lock == claim::owner(&hx(eth(k))).unwrap()
                                            && c.output.type_script.is_none()
                                    })
                                    .unwrap()
                                    .clone()
                            })
                            .collect(),
                    }]
                },
            )
            .unwrap()
        } else {
            tool::admin(
                self,
                &native,
                &self.ledger,
                &d,
                &b,
                op,
                0,
                records,
                vec![self.node.live[&self.sponsor].clone()],
                deps,
                json!([]),
            )
            .unwrap()
        };
        let messages = tool::signing::requests(&d, &sk).unwrap();
        let responses = messages
            .groups
            .iter()
            .map(|g| {
                (1..=3)
                    .find_map(|k| {
                        tool::signing::sign(&messages, &g.lock_hash, &key(k).secret_bytes()).ok()
                    })
                    .expect("Known test signing key")
            })
            .collect::<Vec<_>>();
        if op == s::Operation::Pay && !self.ledger.recipient_scripts.is_empty() {
            if let Ok(path) = std::env::var("GW_CONTROL_BROWSER_FIXTURE_V0") {
                let ops = responses
                    .iter()
                    .filter(|s| {
                        messages.groups.iter().any(|g| {
                            g.kind != "ethereum_personal_sign"
                                && s.signatures.iter().any(|sig| sig.lock_hash == g.lock_hash)
                        })
                    })
                    .collect::<Vec<_>>();
                let approvals = responses
                    .iter()
                    .filter(|s| {
                        messages.groups.iter().any(|g| {
                            g.kind == "ethereum_personal_sign"
                                && s.signatures.iter().any(|sig| sig.lock_hash == g.lock_hash)
                        })
                    })
                    .collect::<Vec<_>>();
                let manifest = hx(self.profile.settlement.manifest);
                let p = json!({"protocol":"v0","plan_manifest":manifest,"control_accounts":[4],"proposal":{"protocol":"v0","plan_manifest":manifest,"skeleton":sk,"signatures":ops},"signing_request":messages});
                std::fs::write(path,serde_json::to_vec_pretty(&json!({"response":p,"approvals":approvals,"owner":hx(eth(1)),"owners":[hx(eth(1)),hx(eth(2)),hx(eth(4))]})).unwrap()).unwrap();
            }
        }
        let signed = tool::signing::assemble(&d, &sk, &responses).unwrap();
        Skeleton {
            transaction: signed,
            ..sk
        }
    }
}
#[test]
fn v0_production_builder_signer_and_omnilock_full_flow() {
    let mut f = fixture();
    let vm = std::env::var_os("GW_V0_SETTLEMENT_BINARY").is_some();
    while let Some((op, records)) = f
        .ledger
        .next_with_profile_hash(hash(&v0::encode_profile(&f.profile)), 4)
        .unwrap()
    {
        let sk = f.production(op, records, None);
        let a = decode_transaction(&sk.transaction);
        f.apply(&a, sk.transaction, vm);
    }
    for (capacity, token) in [(300 * CAP, 40u64), (300 * CAP, 60u64)] {
        let a = f.payment(capacity, token).unwrap();
        let sk = f.production(
            s::Operation::Pay,
            a.records.clone(),
            Some(BTreeMap::from([
                (1, capacity.to_string()),
                (3, token.to_string()),
            ])),
        );
        let a = decode_transaction(&sk.transaction);
        let p = point(&sk.transaction, 1);
        f.apply(&a, sk.transaction, vm);
        let (_, _, _, assets, mut request) = setup_claim();
        request.inputs = vec![p.clone()];
        let q = claim::construct(
            &request,
            vec![f.node.live[&p].clone()],
            &assets,
            &f.deps,
            1000,
        )
        .unwrap();
        let user = claim::assemble(&q, &signature(&q.signing_message, 1)).unwrap();
        if vm {
            let n = f.node.vm(&user).unwrap();
            println!("v0 production CLI builder user exit: {n} cycles");
        }
        f.node.insert(user);
    }
    assert!(f.payment(1, 1).is_err());
}

#[test]
fn v0_provenance_remainder_direct_and_mixed_cell_origins() {
    use godwoken_withdraw::claim::provenance::Provenance;
    let mut f = fixture();
    f.active(false);
    let a = f.payment(600 * CAP, 100).unwrap();
    let tx = f.transaction(&a);
    let payout = point(&tx, 1);
    f.apply(&a, tx, false);
    let d = f.deployment();
    let owner = claim::owner(&hx(eth(1))).unwrap();
    assert!(Provenance::new(&f.node, &d, 1)
        .unwrap()
        .owns(&payout, &owner)
        .unwrap());
    let (_, _, _, assets, mut request) = setup_claim();
    request.inputs = vec![payout.clone()];
    let asset = assets
        .iter()
        .find(|a| a.type_hash == f.token.hash().unwrap())
        .unwrap()
        .asset_id;
    request.amounts = Some(vec![
        claim::ClaimAmount {
            asset_id: 1,
            raw: (250 * CAP).to_string(),
        },
        claim::ClaimAmount {
            asset_id: asset,
            raw: "40".into(),
        },
    ]);
    let q = claim::construct(
        &request,
        vec![f.node.live[&payout].clone()],
        &assets,
        &f.deps,
        1000,
    )
    .unwrap();
    let user = claim::assemble(&q, &signature(&q.signing_message, 1)).unwrap();
    f.node.vm(&user).unwrap();
    let change = user
        .outputs
        .iter()
        .enumerate()
        .filter(|(_, c)| c.lock == owner)
        .map(|(i, _)| point(&user, i))
        .collect::<Vec<_>>();
    f.node.insert(user);
    for p in &change {
        assert!(Provenance::new(&f.node, &d, 1)
            .unwrap()
            .owns(p, &owner)
            .unwrap());
    }
    let direct = base_tx(
        vec![CellOutput {
            capacity: format!("0x{:x}", 200 * CAP),
            lock: owner.clone(),
            type_script: None,
        }],
        vec!["0x".into()],
    );
    let direct_point = point(&direct, 0);
    f.node.insert(direct);
    assert!(!Provenance::new(&f.node, &d, 1)
        .unwrap()
        .owns(&direct_point, &owner)
        .unwrap());
    request.inputs = change.clone();
    request.inputs.push(direct_point);
    request.amounts = Some(vec![claim::ClaimAmount {
        asset_id: 1,
        raw: (100 * CAP).to_string(),
    }]);
    let q = claim::construct(
        &request,
        request
            .inputs
            .iter()
            .map(|p| f.node.live[p].clone())
            .collect(),
        &assets,
        &f.deps,
        1000,
    )
    .unwrap();
    let mixed = claim::assemble(&q, &signature(&q.signing_message, 1)).unwrap();
    f.node.vm(&mixed).unwrap();
    let mixed_change = mixed
        .outputs
        .iter()
        .enumerate()
        .filter(|(_, c)| c.lock == owner)
        .map(|(i, _)| point(&mixed, i))
        .collect::<Vec<_>>();
    f.node.insert(mixed);
    for p in mixed_change {
        assert!(!Provenance::new(&f.node, &d, 1)
            .unwrap()
            .owns(&p, &owner)
            .unwrap());
    }
    let mut wrong = d.clone();
    wrong.validator_data_hash = hx([99; 32]);
    assert!(Provenance::new(&f.node, &wrong, 1)
        .unwrap()
        .owns(&payout, &owner)
        .is_err());
    f.node.orphan = true;
    assert!(Provenance::new(&f.node, &d, 1)
        .unwrap()
        .owns(&payout, &owner)
        .is_err());
}

#[test]
fn control_v0_full_vm_partial_and_remainder() {
    if std::env::var_os("GW_CONTROL_TEST").is_none() {
        return;
    }
    assert!(std::env::var_os("GW_V0_SETTLEMENT_BINARY").is_some());
    let mut f = fixture();
    f.active(true);
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
    f.node.insert(fund);
    for (capacity, token) in [(300 * CAP, 40), (300 * CAP, 60)] {
        let a = f.payment(capacity, token).unwrap();
        let sk = f.production(
            s::Operation::Pay,
            a.records,
            Some(BTreeMap::from([
                (1, capacity.to_string()),
                (3, token.to_string()),
            ])),
        );
        let d = f.deployment();
        let profile = godwoken_withdraw::settlement::adapter::Profile::load(&d).unwrap();
        let a =
            godwoken_withdraw::settlement::replay::action_for(&sk.transaction, &profile).unwrap();
        let mut wrong = sk.transaction.clone();
        let i = sk
            .input_cells
            .iter()
            .position(|c| c.output.lock == claim::owner(&hx(eth(2))).unwrap())
            .unwrap();
        wrong.witnesses[i] = witness(Some(omni_signature(&[0; 65])), None);
        assert!(f.node.vm(&wrong).is_err());
        let payout = point(&sk.transaction, 1);
        f.apply(&a, sk.transaction, true);
        let (_, _, _, assets, mut request) = setup_claim();
        request.inputs = vec![payout.clone()];
        let q = claim::construct(
            &request,
            vec![f.node.live[&payout].clone()],
            &assets,
            &f.deps,
            1000,
        )
        .unwrap();
        let tx = claim::assemble(&q, &signature(&q.signing_message, 1)).unwrap();
        println!(
            "V0 control OmniLock onward exit: {} cycles",
            f.node.vm(&tx).unwrap()
        );
        f.node.insert(tx);
    }
    assert!(f.payment(1, 1).is_err());
}

#[test]
fn multisig_upgrade_v0() {
    let f = fixture();
    let d = f.deployment();
    super::super::upgrade_e2e::exercise(f.node, d);
}

/// Explicit real-DB artifact acceptance. The artifacts contain synthetic L1 authorities,
/// while the native root, beneficiaries, balances and Merkle proofs come from the real DB.
#[test]
fn v0_real_db_cold_artifacts_signed_vm() {
    let Ok(dir) = std::env::var("GW_V0_COLD_ARTIFACTS") else {
        return;
    };
    assert!(std::env::var_os("GW_V0_SETTLEMENT_BINARY").is_some());
    let archive: Value = serde_json::from_slice(
        &std::fs::read(std::path::Path::new(&dir).join("archive.json")).unwrap(),
    )
    .unwrap();
    let mut d: Deployment = serde_json::from_value(archive["deployment"].clone()).unwrap();
    let mut f = fixture();
    deploy(&mut f.node, &mut f.deps, None, bytes(&d.profile).unwrap());
    deploy(
        &mut f.node,
        &mut f.deps,
        None,
        bytes(archive["bundle"]["config_data"].as_str().unwrap()).unwrap(),
    );
    // Collapse identical dependency cells from the two fixture/profile configurations.
    let mut seen = std::collections::BTreeSet::new();
    f.deps.retain(|dep| seen.insert(dep.out_point.clone()));
    d.cell_deps = f.deps.clone();
    let mut results = vec![];
    let mut hashes = BTreeMap::<String, String>::new();
    for step in 0..4 {
        let mut sk: Skeleton = serde_json::from_slice(
            &std::fs::read(std::path::Path::new(&dir).join(format!("step-{step}.json"))).unwrap(),
        )
        .unwrap();
        let original_hash = sk.transaction.hash().unwrap();
        for cell in &mut sk.input_cells {
            if let Some(hash) = hashes.get(&cell.out_point.tx_hash) {
                cell.out_point.tx_hash = hash.clone();
                assert!(
                    withdraw::chain::same_cell(&f.node.live[&cell.out_point], cell),
                    "Signed predecessor output differs"
                );
            } else {
                f.node.live.insert(cell.out_point.clone(), cell.clone());
            }
        }
        for input in &mut sk.transaction.inputs {
            if let Some(hash) = hashes.get(input["previous_output"]["tx_hash"].as_str().unwrap()) {
                input["previous_output"]["tx_hash"] = json!(hash);
            }
        }
        sk.dependency_cells = f
            .deps
            .iter()
            .map(|d| f.node.live[&d.out_point].clone())
            .collect();
        sk.transaction.cell_deps = f
            .deps
            .iter()
            .map(|d| json!({"out_point":d.out_point,"dep_type":"code"}))
            .collect();
        let req = godwoken_withdraw::settlement::signing::requests(&d, &sk).unwrap();
        let sigs = req
            .groups
            .iter()
            .map(|g| {
                (1..=4)
                    .find_map(|n| {
                        godwoken_withdraw::settlement::signing::sign(
                            &req,
                            &g.lock_hash,
                            &key(n).secret_bytes(),
                        )
                        .ok()
                    })
                    .expect("Synthetic rehearsal authority")
            })
            .collect::<Vec<_>>();
        let tx = godwoken_withdraw::settlement::signing::assemble(&d, &sk, &sigs).unwrap();
        let cycles = f.node.vm(&tx).unwrap();
        assert!(cycles < 70_000_000);
        println!(
            "Real DB V0 cold step {step}: {cycles} cycles, {} bytes",
            tx.packed().unwrap().as_slice().len()
        );
        results.push(json!({"step":step,"cycles":cycles,"bytes":tx.packed().unwrap().as_slice().len(),"before_root":sk.before_root,"after_root":sk.after_root,"signed_vm_verified":true}));
        if step > 0 {
            let mut bad = tx.clone();
            bad.outputs[1].capacity =
                format!("0x{:x}", quantity(&bad.outputs[1].capacity).unwrap() + 1);
            assert!(f.node.vm(&bad).is_err());
        }
        hashes.insert(original_hash, tx.hash().unwrap());
        f.node.insert(tx.clone());
        assert!(
            f.node.vm(&tx).is_err(),
            "Consumed signed transaction replayed"
        );
    }
    if let Ok(result_path) = std::env::var("GW_V0_COLD_RESULT") {
        std::fs::write(result_path,serde_json::to_vec_pretty(&json!({"status":"PASS","scope":"Real DB proofs; synthetic L1 inputs and public test-key authorities; no broadcast; end-user keys not used","signed_predecessor_chain_checked":true,"stages":results})).unwrap()).unwrap();
    }
}

#[path = "unified_batches.rs"]
mod unified_batches;
