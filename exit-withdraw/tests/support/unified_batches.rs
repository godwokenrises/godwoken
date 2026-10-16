//! Single-root authorization checks and opt-in, reproducible large-state VM benchmarks.
use super::*;
use contract::settlement::physical;
use std::collections::BTreeSet;

impl Fixture {
    fn batch(&self, records: Vec<Vec<u8>>) -> Skeleton {
        let mut owners = BTreeMap::<String, BTreeMap<u32, U256>>::new();
        for raw in &records {
            let pay: Payment = decode(raw).unwrap();
            let source = self.ledger.source(pay.claim.source).unwrap();
            *owners
                .entry(hx(pay.claim.address))
                .or_default()
                .entry(source.asset)
                .or_default() += pay.amount;
        }
        let requests = owners
            .into_iter()
            .map(|(eth_address, amounts)| Request::Amounts {
                eth_address,
                amounts: amounts
                    .into_iter()
                    .map(|(id, n)| (id, n.to_string()))
                    .collect(),
            })
            .collect();
        self.production_requests(s::Operation::Pay, records, Some(requests))
    }
}

fn target_claims(f: &Fixture) -> Vec<Claim> {
    let mut claims: Vec<_> = f.ledger.records[s::CLAIM_PLAN]
        .values()
        .map(|v| decode::<Claim>(v).unwrap())
        .filter(|c| f.ledger.source(c.source).unwrap().account < 45)
        .collect();
    claims.sort_by_key(|c| (f.ledger.source(c.source).unwrap().asset, c.lot));
    claims
}

fn check_transaction(
    f: &Fixture,
    records: Vec<Vec<u8>>,
    label: &str,
    mature: bool,
) -> (Skeleton, s::SettlementAction, Result<u64>) {
    let sk = f.batch(records);
    let a = decode_transaction(&sk.transaction);
    assert_eq!(a.trees.len(), 1);
    assert_eq!(a.trees[0].kind, physical::STATE);
    let size = sk.transaction.packed().unwrap().as_slice().len();
    let largest = sk
        .transaction
        .witnesses
        .iter()
        .map(|w| bytes(w).unwrap().len())
        .max()
        .unwrap();
    assert!(size <= 512_000 && largest <= 32_768);
    let sources: BTreeSet<_> = a
        .records
        .iter()
        .map(|raw| {
            let p: Payment = decode(raw).unwrap();
            f.ledger.source(p.claim.source).unwrap().account
        })
        .collect();
    let cycles = f.node.vm(&sk.transaction);
    println!(
        "UNIFIED_BENCH {}",
        json!({"case":label,"mature":mature,"records":a.records.len(),"sources":sources.len(),"recipients":a.native.recipients.len(),"bytes":size,"max_witness":largest,"state_roots":a.trees.len(),"state_leaves":a.trees[0].leaves.len(),"proof_bytes":a.trees[0].proof.0.len(),"cycles":cycles.as_ref().ok(),"error":cycles.as_ref().err().map(|e|e.to_string())})
    );
    (sk, a, cycles)
}

fn reject_malformed_chunks(f: &Fixture, tx: &Transaction) {
    let first = tx.inputs.len();
    assert!(tx.witnesses.len() > first + 1);
    let mut variants = Vec::new();
    let mut bad = tx.clone();
    let mut part = bytes(&bad.witnesses[first]).unwrap();
    part[0] ^= 1;
    bad.witnesses[first] = hx(part);
    variants.push(("corrupt chunk", bad));
    let mut bad = tx.clone();
    bad.witnesses.pop();
    variants.push(("missing chunk", bad));
    let mut bad = tx.clone();
    bad.witnesses.swap(first, first + 1);
    variants.push(("reordered chunks", bad));
    let mut bad = tx.clone();
    bad.witnesses[first + 1] = bad.witnesses[first].clone();
    variants.push(("duplicated chunk", bad));
    let mut bad = tx.clone();
    let w = packed::WitnessArgs::from_slice(&bytes(&bad.witnesses[0]).unwrap()).unwrap();
    let mut header = w.output_type().to_opt().unwrap().raw_data().to_vec();
    header[12..16].copy_from_slice(&1u32.to_le_bytes());
    bad.witnesses[0] = hx(w
        .as_builder()
        .output_type(Some(gw_types::bytes::Bytes::from(header)).pack())
        .build()
        .as_slice());
    variants.push(("wrong chunk count", bad));
    for (name, mut bad) in variants {
        // Re-sign so rejection must come from consensus payload checks, not a stale signature.
        resign(f, &mut bad);
        let error = f.node.vm(&bad).expect_err(name).to_string();
        assert!(
            error.contains("Type") && error.contains("ValidationFailure"),
            "{name}: wrong rejection: {error}"
        );
        println!("UNIFIED_CHUNK_REJECTED {name}");
    }
}

fn assert_drained(f: &Fixture, claims: &[Claim], expected_sources: usize) {
    let mut source_assets = BTreeSet::new();
    let mut owners = BTreeSet::new();
    for claim in claims {
        let source = f.ledger.source(claim.source).unwrap();
        if source_assets.insert((source.account, source.asset)) {
            assert_eq!(
                f.native
                    .get(
                        &NativeVersion::V0
                            .balance_key(source.asset, source.address)
                            .into()
                    )
                    .unwrap(),
                gw_smt::smt::SMTH256::zero()
            );
        }
        owners.insert(claim.address);
        assert_eq!(
            f.ledger
                .paid(f.profile.settlement.deployment, claim)
                .unwrap(),
            claim.cap
        );
    }
    assert_eq!(
        source_assets
            .iter()
            .map(|(s, _)| *s)
            .collect::<BTreeSet<_>>()
            .len(),
        expected_sources
    );
    for owner in owners {
        for asset in [1, 3] {
            assert_eq!(
                f.native
                    .get(&NativeVersion::V0.balance_key(asset, owner).into())
                    .unwrap(),
                gw_smt::smt::SMTH256::zero()
            );
        }
    }
    println!(
        "UNIFIED_COMPLETE claims={} source_asset_pairs={}",
        claims.len(),
        source_assets.len()
    );
}

// This is a consistent authenticated input-state fixture, not a replay of 20,000 historical transactions.
fn seed_history(f: &mut Fixture) {
    let d = f.profile.settlement.deployment;
    let claims: Vec<_> = f.ledger.records[s::CLAIM_PLAN]
        .values()
        .map(|v| decode::<Claim>(v).unwrap())
        .filter(|c| f.ledger.source(c.source).unwrap().account >= 45)
        .collect();
    for claim in &claims {
        let source = f.ledger.source(claim.source).unwrap();
        f.ledger.records[s::PAID].insert(
            claim_key(d, claim),
            encode(&Paid {
                amount: claim.cap,
                ..Default::default()
            }),
        );
        f.ledger.records[s::RESERVES].insert(
            claim.lot,
            encode(&Reserve {
                remaining: U256::zero(),
            }),
        );
        f.ledger.records[s::SOURCES].insert(
            claim.source,
            encode(&SourceBudget {
                paid: claim.cap,
                ..Default::default()
            }),
        );
        f.native
            .update(
                NativeVersion::V0
                    .balance_key(source.asset, source.address)
                    .into(),
                contract::value(U256::zero()).into(),
            )
            .unwrap();
    }
    let key = asset_key(d, 1);
    let mut asset: AssetBudget = decode(&f.ledger.records[s::ASSETS][&key]).unwrap();
    asset.paid = U256::from(claims.len());
    f.ledger.records[s::ASSETS].insert(key, encode(&asset));
    f.ledger.rebuild_from_records().unwrap();
    let mut m = f.ledger.metadata.unwrap();
    m.state_root = f.ledger.state_root();
    m.sequence += 313;
    f.ledger.metadata = Some(m);
    f.native
        .update(
            metadata_key(d, contract::script_hash(&f.profile.settlement.base.rollup)).into(),
            commitment(&encode(&m)).into(),
        )
        .unwrap();
    let cell = f.node.live.get_mut(&f.roll).unwrap();
    let state = packed::GlobalState::from_slice(&bytes(&cell.output_data).unwrap()).unwrap();
    let account = state
        .account()
        .as_builder()
        .merkle_root(packed::Byte32::from_slice(f.native.root().as_slice()).unwrap())
        .build();
    cell.output_data = hx(state.as_builder().account(account).build().as_slice());
    let point = f
        .custody
        .iter()
        .find(|p| f.node.live[*p].output.type_script.is_none())
        .unwrap()
        .clone();
    let cell = f.node.live.get_mut(&point).unwrap();
    cell.output.capacity = format!(
        "0x{:x}",
        quantity(&cell.output.capacity).unwrap() - claims.len() as u64
    );
}

fn export_profile(paged: bool) {
    let f = fixture_scaled(true, paged);
    let path = std::env::var("GW_V0_SETTLEMENT_PROFILE").expect("Set GW_V0_SETTLEMENT_PROFILE");
    let raw = v0::encode_profile(&f.profile);
    std::fs::write(&path, &raw).unwrap();
    std::fs::write(format!("{path}.hash"), hex::encode(hash(&raw))).unwrap();
}

#[test]
#[ignore = "large benchmark profile; run scripts/test-unified-batch-e2e.sh"]
fn export_batch_profile() {
    export_profile(false);
}
#[test]
#[ignore = "paged benchmark profile; run scripts/test-unified-batch-e2e.sh paged"]
fn export_paged_profile() {
    export_profile(true);
}

#[test]
#[ignore = "full signed VM benchmark; run scripts/test-unified-batch-e2e.sh"]
fn twenty_account_batches() {
    assert!(std::env::var_os("GW_V0_SETTLEMENT_BINARY").is_some());
    for mature in [false, true] {
        let mut f = fixture_scaled(true, false);
        f.active(true);
        if mature {
            seed_history(&mut f);
        }
        let claims = target_claims(&f);
        assert_eq!(claims.len(), 51);
        for n in [20, 40, 51] {
            let records = claims
                .iter()
                .take(n)
                .map(|c| {
                    encode(&Payment {
                        claim: *c,
                        amount: c.cap,
                    })
                })
                .collect();
            let (sk, a, cycles) = check_transaction(&f, records, &format!("single_{n}"), mature);
            assert_eq!(a.native.recipients.len(), 20);
            if n == 20 && !mature {
                reject_malformed_chunks(&f, &sk.transaction);
            }
            if let Err(e) = cycles {
                assert!(
                    n > 20 && e.to_string().contains("ExceededMaximumCycles"),
                    "{e}"
                );
            }
        }
        let first: BTreeSet<_> = claims
            .iter()
            .map(|c| c.address)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .take(10)
            .collect();
        for group in [true, false] {
            let records = claims
                .iter()
                .filter(|c| first.contains(&c.address) == group)
                .map(|c| {
                    encode(&Payment {
                        claim: *c,
                        amount: c.cap,
                    })
                })
                .collect();
            let (sk, a, cycles) = check_transaction(
                &f,
                records,
                if group { "owners_1" } else { "owners_2" },
                mature,
            );
            assert!(cycles.unwrap() <= 70_000_000);
            f.apply(&a, sk.transaction, false);
        }
        assert_drained(&f, &claims, 20);
    }
}

#[test]
#[ignore = "full signed paged VM benchmark; run scripts/test-unified-batch-e2e.sh paged"]
fn paged_account_batch() {
    assert!(std::env::var_os("GW_V0_SETTLEMENT_BINARY").is_some());
    let mut f = fixture_scaled(true, true);
    f.active(true);
    let claims = target_claims(&f);
    assert_eq!(claims.len(), 51);
    let records = claims
        .iter()
        .map(|c| {
            encode(&Payment {
                claim: *c,
                amount: c.cap,
            })
        })
        .collect();
    let (sk, a, cycles) = check_transaction(&f, records, "one_source_paged", false);
    assert!(
        a.trees[0]
            .leaves
            .iter()
            .filter(|(key, _)| physical::namespace(*key) == Ok(16))
            .count()
            > 1
    );
    assert!(cycles.unwrap() <= 70_000_000);
    f.apply(&a, sk.transaction, false);
    assert_drained(&f, &claims, 1);
}

#[test]
fn immutable_plan_and_unrelated_counter_changes_are_rejected() {
    let mut f = fixture();
    let vm = std::env::var_os("GW_V0_SETTLEMENT_BINARY").is_some();
    f.active(vm);
    let claims = target_claims(&f);
    let records = claims
        .iter()
        .map(|c| {
            encode(&Payment {
                claim: *c,
                amount: if f.ledger.source(c.source).unwrap().asset == 1 {
                    U256::from(300 * CAP)
                } else {
                    U256::from(40)
                },
            })
        })
        .collect();
    let sk = f.batch(records);
    let a = decode_transaction(&sk.transaction);
    // All three static record classes are now inside the same authenticated account leaf.
    for kind in [s::SOURCE_PLAN, s::LOT_PLAN, s::CLAIM_PLAN] {
        let mut bad = a.clone();
        let (_, raw) = bad.trees[0]
            .leaves
            .iter_mut()
            .find(|(k, _)| physical::namespace(*k) == Ok(16))
            .unwrap();
        let mut page = physical::unpack_page(raw).unwrap();
        let value = page.iter_mut().find(|((k, _), _)| *k == kind).unwrap().1;
        let last = value.len() - 1;
        value[last] ^= 1;
        *raw = physical::pack_page(&page);
        rejected(&f, "immutable account record", sk.transaction.clone(), bad);
    }
    let mut bad = a.clone();
    let mut m: Metadata = decode(&bad.after).unwrap();
    m.state_root[0] ^= 1;
    bad.after = encode(&m);
    rejected(
        &f,
        "arbitrary output state root",
        sk.transaction.clone(),
        bad,
    );
    f.apply(&a, sk.transaction, vm);
    let token = claims
        .iter()
        .find(|c| f.ledger.source(c.source).unwrap().asset == 3)
        .unwrap();
    let protected = [
        (s::PAID, claim_key(f.profile.settlement.deployment, token)),
        (s::RESERVES, token.lot),
        (s::SOURCES, token.source),
    ];
    let saved: Vec<_> = protected
        .iter()
        .map(|(kind, key)| f.ledger.records[*kind][key].clone())
        .collect();
    let ckb = claims
        .iter()
        .find(|c| f.ledger.source(c.source).unwrap().asset == 1)
        .unwrap();
    let sk = f.batch(vec![encode(&Payment {
        claim: *ckb,
        amount: U256::from(300 * CAP),
    })]);
    let a = decode_transaction(&sk.transaction);
    let mut bad = a.clone();
    let (_, raw) = bad.trees[0]
        .leaves
        .iter_mut()
        .find(|(k, _)| physical::namespace(*k) == Ok(16))
        .unwrap();
    let mut page = physical::unpack_page(raw).unwrap();
    page.get_mut(&protected[0]).unwrap()[4] ^= 1;
    *raw = physical::pack_page(&page);
    rejected(&f, "unrelated token counter", sk.transaction.clone(), bad);
    f.apply(&a, sk.transaction, vm);
    for ((kind, key), before) in protected.iter().zip(&saved) {
        assert_eq!(&f.ledger.records[*kind][key], before);
    }
}
