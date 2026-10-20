//! Claims use real deployed OmniLock/sUDT bytecode and CKB-VM; no recovery binary needed.
use super::*;
use bech32::{ToBase32, Variant};
use godwoken_withdraw::claim::{self, Asset, BuildRequest};
pub(super) fn setup_claim() -> (Node, Vec<Dependency>, Vec<Cell>, Vec<Asset>, BuildRequest) {
    let mut node = Node::default();
    let mut deps = vec![];
    for path in [
        "tests/fixtures/mainnet-evidence/eth-lock-research/standard-omnilock-live.json",
        "tests/fixtures/mainnet-sudt-live.json",
    ] {
        let v: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        let r = if v.get("response").is_some() {
            &v["response"]
        } else {
            &v
        };
        let c = &r["result"]["cell"];
        let ty: Script = serde_json::from_value(c["output"]["type"].clone()).unwrap();
        let data = c["data"]["content"].as_str().unwrap().to_owned();
        let tx = base_tx(
            vec![CellOutput {
                capacity: format!("0x{:x}", 1_000_000 * CAP),
                lock: script([0; 32], vec![]),
                type_script: Some(ty),
            }],
            vec![data.clone()],
        );
        deps.push(Dependency {
            out_point: point(&tx, 0),
            dep_type: "code".into(),
            data_hash: hx(hash(&bytes(&data).unwrap())),
        });
        node.insert(tx);
    }
    let data = ckb_system_scripts::BUNDLED_CELL
        .get("specs/cells/secp256k1_data")
        .unwrap();
    let tx = base_tx(
        vec![CellOutput {
            capacity: format!("0x{:x}", 1_000_000 * CAP),
            lock: script([0; 32], vec![]),
            type_script: None,
        }],
        vec![hx(&data)],
    );
    deps.push(Dependency {
        out_point: point(&tx, 0),
        dep_type: "code".into(),
        data_hash: hx(hash(&data)),
    });
    node.insert(tx);
    let catalogue: Vec<Asset> =
        serde_json::from_slice(&std::fs::read("tests/fixtures/settlement/assets.json").unwrap())
            .unwrap();
    let evidence: Value = serde_json::from_slice(
        &std::fs::read("tests/fixtures/mainnet-evidence/v1-custodian_lock-page-0.json").unwrap(),
    )
    .unwrap();
    let types: Vec<Script> = evidence["response"]["result"]["objects"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|c| serde_json::from_value::<Script>(c["output"]["type"].clone()).ok())
        .filter(|t| catalogue.iter().any(|a| a.type_hash == t.hash().unwrap()))
        .take(2)
        .collect();
    assert_eq!(types.len(), 2);
    let lock = claim::owner(&hx(eth(1))).unwrap();
    let outputs = vec![
        CellOutput {
            capacity: format!("0x{:x}", 1000 * CAP),
            lock: lock.clone(),
            type_script: None,
        },
        CellOutput {
            capacity: format!("0x{:x}", 200 * CAP),
            lock: lock.clone(),
            type_script: Some(types[0].clone()),
        },
        CellOutput {
            capacity: format!("0x{:x}", 200 * CAP),
            lock,
            type_script: Some(types[1].clone()),
        },
    ];
    let funding = base_tx(
        outputs,
        vec![
            "0x".into(),
            hx(123456789u128.to_le_bytes()),
            hx(987654321u128.to_le_bytes()),
        ],
    );
    let inputs = (0..3)
        .map(|i| funding.cell(&point(&funding, i)).unwrap())
        .collect::<Vec<_>>();
    node.insert(funding);
    let destination = claim::address(&script(
        contract::MULTISIG,
        hash(&multisig(5))[..20].to_vec(),
    ))
    .unwrap();
    let r = BuildRequest {
        address: hx(eth(1)),
        destination,
        inputs: inputs.iter().map(|c| c.out_point.clone()).collect(),
        amounts: None,
    };
    (node, deps, inputs, catalogue, r)
}
pub(super) fn signature(message: &str, n: u8) -> String {
    let msg = claim::personal_digest(hash32(message).unwrap());
    let (id, s) = Secp256k1::new()
        .sign_ecdsa_recoverable(&Message::from_digest(msg), &key(n))
        .serialize_compact();
    let mut s = s.to_vec();
    s.push(i32::from(id) as u8 + 27);
    hx(s)
}
#[test]
fn claim_portal_signed_multi_asset_vm_and_cross_language_fixture() {
    let (mut node, deps, inputs, assets, r) = setup_claim();
    let q = claim::construct(&r, inputs.clone(), &assets, &deps, 1000).unwrap();
    let sig = signature(&q.signing_message, 1);
    let tx = claim::assemble(&q, &sig).unwrap();
    let cycles = node.vm(&tx).unwrap();
    assert!(cycles < 70_000_000);
    println!("User portal full signed OmniLock + sUDT transaction: {cycles} cycles");
    assert_eq!(q.transaction_hash, tx.hash().unwrap());
    assert_eq!(
        tx.outputs
            .iter()
            .map(|o| quantity(&o.capacity).unwrap())
            .sum::<u64>()
            + q.fee_shannons.parse::<u64>().unwrap(),
        1400 * CAP
    );
    for (o, d) in tx.outputs.iter().zip(&tx.outputs_data) {
        assert_eq!(o.lock, claim::parse_address(&r.destination).unwrap());
        assert!(quantity(&o.capacity).unwrap() >= claim::occupied(o, d).unwrap());
    }
    let mut bad = tx.clone();
    bad.outputs[0].capacity = format!("0x{:x}", quantity(&bad.outputs[0].capacity).unwrap() - 1);
    assert!(node.vm(&bad).is_err());
    let mut bad = tx.clone();
    bad.outputs[0].lock.args = hx([8; 20]);
    assert!(node.vm(&bad).is_err());
    let mut bad = tx.clone();
    bad.outputs_data[0] = hx(1u128.to_le_bytes());
    assert!(node.vm(&bad).is_err());
    if let Some(path) = std::env::var_os("GW_CLAIM_FIXTURE") {
        std::fs::write(path,serde_json::to_vec_pretty(&json!({"request":r,"quote":q,"signature":sig,"config":{"network":"CKB mainnet","scope":"Godwoken v1","assets":assets,"dependencies":deps,"broadcast_enabled":true,"fee_rate":1000,"plan_manifest":hx([0;32]),"snapshot_root":hx([0;32])}})).unwrap()).unwrap();
    }
    node.insert(tx.clone());
    assert!(node.vm(&tx).is_err());
}
#[test]
fn claim_portal_rejects_wrong_owner_signature_assets_and_capacity() {
    let (_, deps, inputs, assets, r) = setup_claim();
    let q = claim::construct(&r, inputs.clone(), &assets, &deps, 1000).unwrap();
    assert!(claim::assemble(&q, &signature(&q.signing_message, 2)).is_err());
    assert!(claim::assemble(&q, "0x00").is_err());
    let mut invalid = bytes(&signature(&q.signing_message, 1)).unwrap();
    invalid[64] = 37;
    assert!(claim::assemble(&q, &hx(invalid)).is_err());
    let mut wrong = r.clone();
    wrong.address = hx(eth(2));
    assert!(claim::construct(&wrong, inputs.clone(), &assets, &deps, 1000).is_err());
    let mut cells = inputs.clone();
    cells[1].output.lock.args.push_str("00");
    assert!(claim::construct(&r, cells, &assets, &deps, 1000).is_err());
    let mut cells = inputs.clone();
    cells[1].output.type_script.as_mut().unwrap().args = hx([66; 32]);
    assert!(claim::construct(&r, cells, &assets, &deps, 1000).is_err());
    let mut cells = inputs.clone();
    cells[1].output_data.push_str("00");
    assert!(claim::construct(&r, cells, &assets, &deps, 1000).is_err());
    let mut duplicate = r.clone();
    duplicate.inputs[1] = duplicate.inputs[0].clone();
    let mut cells = inputs.clone();
    cells[1] = cells[0].clone();
    assert!(claim::construct(&duplicate, cells, &assets, &deps, 1000).is_err());
    let mut huge = r.clone();
    huge.destination = claim::address(&script(contract::MULTISIG, vec![3; 1600])).unwrap();
    assert!(claim::construct(&huge, inputs.clone(), &assets, &deps, 1000).is_err());
    let mut dust = inputs[0].clone();
    dust.output.capacity = format!("0x{:x}", claim::occupied(&dust.output, "0x").unwrap());
    let mut req = r.clone();
    req.inputs = vec![dust.out_point.clone()];
    req.destination = claim::address(&script(contract::MULTISIG, vec![5; 22])).unwrap();
    assert!(claim::construct(&req, vec![dust], &assets, &deps, 1000).is_err());
    let mut same = r.clone();
    same.destination = claim::address(&claim::owner(&r.address).unwrap()).unwrap();
    assert!(claim::construct(&same, inputs, &assets, &deps, 1000).is_err());
}
#[test]
fn claim_portal_addresses_and_plain_ckb() {
    let (_, deps, inputs, assets, mut r) = setup_claim();
    for (format, variant) in [
        (0, Variant::Bech32m),
        (2, Variant::Bech32),
        (4, Variant::Bech32),
    ] {
        let mut b = vec![format];
        b.extend_from_slice(&[7; 32]);
        if format == 0 {
            b.push(1);
        }
        b.extend_from_slice(&[8; 20]);
        let a = bech32::encode("ckb", b.to_base32(), variant).unwrap();
        assert!(claim::parse_address(&a).is_ok());
        let test = bech32::encode("ckt", b.to_base32(), variant).unwrap();
        assert!(claim::parse_address(&test).is_err());
        let wrong = bech32::encode(
            "ckb",
            b.to_base32(),
            if variant == Variant::Bech32 {
                Variant::Bech32m
            } else {
                Variant::Bech32
            },
        )
        .unwrap();
        assert!(claim::parse_address(&wrong).is_err());
    }
    for index in 0..3 {
        let mut b = vec![1, index];
        b.extend_from_slice(&[4; 20]);
        assert!(claim::parse_address(
            &bech32::encode("ckb", b.to_base32(), Variant::Bech32).unwrap()
        )
        .is_ok());
    }
    assert!(claim::parse_address("0x0000000000000000000000000000000000000000").is_err());
    assert!(claim::parse_address(&(r.destination.clone() + "q")).is_err());
    r.inputs.truncate(1);
    let q = claim::construct(&r, vec![inputs[0].clone()], &assets, &deps, 1000).unwrap();
    assert_eq!(q.transaction.outputs.len(), 1);
    assert!(q.transaction.outputs[0].type_script.is_none());
}

#[test]
fn claim_portal_actual_http_build_sign_submit_and_replay_rejection() {
    let dir = tempfile::tempdir().unwrap();
    let (mut setup, plan) = super::settlement_e2e::fixture(dir.path());
    let db = Database::open(&dir.path().join("db")).unwrap();
    super::settlement_e2e::active(&mut setup, &plan, &db, true);
    let sk = godwoken_withdraw::settlement::prepare(
        &db,
        &setup.node,
        &setup.d,
        &setup.b,
        &plan,
        Some(&[hx(eth(1))]),
        4,
    )
    .unwrap();
    let tx = super::settlement_e2e::sign(&sk);
    setup.node.vm(&tx).unwrap();
    super::settlement_e2e::advance_settlement(&mut setup, tx.clone());
    let inputs = vec![setup.node.live[&point(&tx, 1)].clone()];
    let (_, _, _, _, mut r) = setup_claim();
    r.inputs = inputs.iter().map(|c| c.out_point.clone()).collect();
    r.amounts = Some(vec![
        claim::ClaimAmount {
            asset_id: 1,
            raw: (200 * CAP).to_string(),
        },
        claim::ClaimAmount {
            asset_id: 3,
            raw: "40".into(),
        },
    ]);
    let cfg = Config::parse(&bytes(&setup.b.config_data).unwrap()).unwrap();
    let deps = setup
        .d
        .cell_deps
        .iter()
        .filter(|d| {
            let cell = &setup.node.live[&d.out_point];
            let ty = cell.output.type_script.as_ref().and_then(|s| s.hash().ok());
            ty == Some(hx(contract::OMNI))
                || ty == Some(cfg.hashes["l1_sudt"].clone())
                || d.data_hash
                    == "0x9799bee251b975b82c45a02154ce28cec89c5853ecc14d12b7b8cccfc19e0af4"
        })
        .cloned()
        .collect::<Vec<_>>();
    let deploy = dir.path().join("deployment.json");
    let planpath = dir.path().join("plan.json");
    let tables = dir.path().join("tables");
    std::fs::write(&deploy, serde_json::to_vec(&setup.d).unwrap()).unwrap();
    std::fs::write(&planpath, serde_json::to_vec(&plan).unwrap()).unwrap();
    std::fs::write(tables.join("accounts.json"), "[]").unwrap();
    let (unrelated, _, direct, _, _) = setup_claim();
    let direct_point = direct[0].out_point.clone();
    setup
        .node
        .insert(unrelated.txs[&direct_point.tx_hash].clone());
    let rpc = Server::new(setup.node);
    std::fs::write(dir.path().join("index.html"), "Claim test").unwrap();
    let deps_path = dir.path().join("dependencies.json");
    std::fs::write(&deps_path, serde_json::to_vec(&deps).unwrap()).unwrap();
    let scoped =
        |r: &BuildRequest| json!({"protocol":"v1","plan_manifest":plan.manifest,"request":r});
    let socket = TcpListener::bind("127.0.0.1:0").unwrap();
    let listen = socket.local_addr().unwrap().to_string();
    drop(socket);
    let origin = format!("http://{listen}");
    struct Stop(std::process::Child);
    impl Drop for Stop {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let _child = Stop(
        Command::new(env!("CARGO_BIN_EXE_gw-withdraw"))
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
                planpath.to_str().unwrap(),
                "--deployment",
                deploy.to_str().unwrap(),
                "--web-dir",
                dir.path().to_str().unwrap(),
                "--dependencies",
                deps_path.to_str().unwrap(),
                "--enable-broadcast",
            ])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::inherit())
            .spawn()
            .unwrap(),
    );
    for _ in 0..300 {
        if std::net::TcpStream::connect(&listen).is_ok() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    let client = reqwest::blocking::Client::builder()
        .no_proxy()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .unwrap();
    let call = |path: &str, body: Option<Value>, correct_origin: bool| -> Value {
        let url = format!("{origin}{path}");
        let request = match body {
            Some(body) => client.post(url).json(&body),
            None => client.get(url),
        };
        request
            .header(
                "Origin",
                if correct_origin {
                    &origin
                } else {
                    "https://untrusted.invalid"
                },
            )
            .send()
            .unwrap()
            .json()
            .unwrap()
    };
    assert_eq!(
        call("/api/v1/config", None, true)["broadcast_enabled"],
        true
    );
    let inventory = call(&format!("/api/v1/account/{}", r.address), None, true);
    assert!(inventory.get("error").is_none(), "{inventory}");
    assert_eq!(inventory["cells"].as_array().unwrap().len(), 1);
    assert!(call("/api/v1/prepare", Some(scoped(&r)), false)
        .get("error")
        .is_some());
    assert!(inventory["unattributed_cells"].as_u64().unwrap() > 0);
    let mut mixed = r.clone();
    mixed.inputs.push(direct_point);
    assert!(call("/api/v1/prepare", Some(scoped(&mixed)), true)
        .get("error")
        .is_some());
    let mut crossed = scoped(&r);
    crossed["protocol"] = json!("v0");
    assert!(call("/api/v1/prepare", Some(crossed), true)
        .get("error")
        .is_some());
    let mut stale = scoped(&r);
    stale["plan_manifest"] = json!(hx([99; 32]));
    assert!(call("/api/v1/prepare", Some(stale), true)
        .get("error")
        .is_some());
    let quote = call("/api/v1/prepare", Some(scoped(&r)), true);
    assert!(quote.get("error").is_none(), "{quote}");
    let mut changed = r.clone();
    changed.address = hx(eth(2));
    assert!(call("/api/v1/prepare", Some(scoped(&changed)), true)
        .get("error")
        .is_some());
    let body = json!({"request":scoped(&r),"transaction_hash":quote["transaction_hash"],"signing_message":quote["signing_message"],"signature":signature(quote["signing_message"].as_str().unwrap(),1)});
    let mut wrong = body.clone();
    wrong["signature"] = json!(signature(quote["signing_message"].as_str().unwrap(), 2));
    assert!(call("/api/v1/submit", Some(wrong), true)
        .get("error")
        .is_some());
    assert!(inputs
        .iter()
        .all(|c| rpc.node.lock().unwrap().live.contains_key(&c.out_point)));
    let result = call("/api/v1/submit", Some(body.clone()), true);
    assert_eq!(result["status"], "submitted", "{result}");
    assert!(result["cycles"].as_u64().unwrap() < claim::MAX_CYCLES);
    assert!(call("/api/v1/submit", Some(body), true)
        .get("error")
        .is_some());
    let inventory = call(&format!("/api/v1/account/{}", r.address), None, true);
    let remaining: Vec<Cell> = serde_json::from_value(inventory["cells"].clone()).unwrap();
    assert_eq!(remaining.len(), 1);
    r.inputs = remaining.iter().map(|c| c.out_point.clone()).collect();
    r.amounts.as_mut().unwrap()[1].raw = "61".into();
    let rejected = call("/api/v1/prepare", Some(scoped(&r)), true);
    assert!(
        rejected["error"]
            .as_str()
            .unwrap()
            .contains("exceeds remaining balance"),
        "{rejected}"
    );
    assert!(remaining
        .iter()
        .all(|c| rpc.node.lock().unwrap().live.contains_key(&c.out_point)));
    r.amounts = None;
    let quote2 = call("/api/v1/prepare", Some(scoped(&r)), true);
    assert!(quote2.get("error").is_none(), "{quote2}");
    let body2 = json!({"request":scoped(&r),"transaction_hash":quote2["transaction_hash"],"signing_message":quote2["signing_message"],"signature":signature(quote2["signing_message"].as_str().unwrap(),1)});
    let result = call("/api/v1/submit", Some(body2.clone()), true);
    assert_eq!(result["status"], "submitted", "{result}");
    assert!(result["cycles"].as_u64().unwrap() < claim::MAX_CYCLES);
    assert!(call("/api/v1/submit", Some(body2), true)
        .get("error")
        .is_some());
    assert!(
        call(&format!("/api/v1/account/{}", r.address), None, true)["cells"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn standalone_wallet_manual_deposit_sign_submit_change_and_fee() {
    // An ordinary deposit with no refund plan or settlement ancestry.
    let (mut node, deps, original, _, mut request) = setup_claim();
    let old_hash = original[0].out_point.tx_hash.clone();
    let mut funding = node.txs[&old_hash].clone();
    // This standard sUDT does not appear in any refund metadata catalogue.
    funding.outputs[1].type_script.as_mut().unwrap().args = hx([0xab; 32]);
    node.live.retain(|p, _| p.tx_hash != old_hash);
    request.inputs = (0..funding.outputs.len())
        .map(|i| point(&funding, i))
        .collect();
    node.insert(funding);
    let rpc = Server::new(node);
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("index.html"), "Wallet test").unwrap();
    let dep_path = dir.path().join("deps.json");
    std::fs::write(&dep_path, serde_json::to_vec(&deps).unwrap()).unwrap();
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
    let mut child = Stop(
        Command::new(env!("CARGO_BIN_EXE_gw-withdraw"))
            .args([
                "wallet-serve",
                "--listen",
                &listen,
                "--public-origin",
                &origin,
                "--ckb-rpc",
                &rpc.url,
                "--web-dir",
                dir.path().to_str().unwrap(),
                "--dependencies",
                dep_path.to_str().unwrap(),
                "--enable-broadcast",
            ])
            .spawn()
            .unwrap(),
    );
    for _ in 0..300 {
        assert!(child.0.try_wait().unwrap().is_none());
        if std::net::TcpStream::connect(&listen).is_ok() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    let call = |path: &str, body: Option<Value>, valid_origin: bool| -> Value {
        let mut cmd = Command::new("curl");
        cmd.args([
            "-sS",
            "--max-time",
            "30",
            "-H",
            "Content-Type: application/json",
            "-H",
            &format!(
                "Origin: {}",
                if valid_origin {
                    &origin
                } else {
                    "https://other.example"
                }
            ),
        ]);
        if let Some(body) = body {
            cmd.args(["--data", &body.to_string()]);
        }
        let output = cmd
            .arg(format!("{origin}/api/wallet/{path}"))
            .output()
            .unwrap();
        serde_json::from_slice(&output.stdout).unwrap()
    };
    let inventory = call(&format!("account/{}", request.address), None, true);
    assert_eq!(
        inventory["cells"].as_array().unwrap().len(),
        3,
        "{inventory}"
    );
    assert_eq!(inventory["balances"][0]["raw"], (1400 * CAP).to_string());
    assert!(inventory["assets"]
        .as_array()
        .unwrap()
        .iter()
        .any(|a| a["symbol"].is_null()));
    assert!(
        !call(&format!("transactions/{}", request.address), None, true)["transactions"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(call("prepare", Some(json!(request)), false)
        .get("error")
        .is_some());
    let mut foreign = request.clone();
    foreign.address = hx(eth(2));
    assert!(call("prepare", Some(json!(foreign)), true)
        .get("error")
        .is_some());
    request.amounts = Some(vec![claim::ClaimAmount {
        asset_id: 1,
        raw: (200 * CAP).to_string(),
    }]);
    let quote = call("prepare", Some(json!(request)), true);
    assert!(quote.get("error").is_none(), "{quote}");
    let q: claim::Quote = serde_json::from_value(quote.clone()).unwrap();
    let size = q.transaction.packed().unwrap().as_slice().len() as u64 + 4;
    assert_eq!(q.fee_shannons, size.to_string()); // 1000 shannons/KB, rounded up.
    assert!(size < 10000); // Below 0.0001 CKB for this real multi-asset transaction.
    let body = json!({"request":request,"transaction_hash":q.transaction_hash,"signing_message":q.signing_message,"signature":signature(&q.signing_message,1)});
    let mut wrong = body.clone();
    wrong["signature"] = json!(signature(&q.signing_message, 2));
    assert!(call("submit", Some(wrong), true).get("error").is_some());
    let result = call("submit", Some(body.clone()), true);
    assert_eq!(result["status"], "submitted", "{result}");
    assert!(result["cycles"].as_u64().unwrap() < claim::MAX_CYCLES);
    assert!(call("submit", Some(body), true).get("error").is_some());
    let remaining = call(&format!("account/{}", request.address), None, true);
    assert_eq!(
        remaining["balances"][0]["raw"],
        (1200 * CAP - size).to_string()
    );
    assert_eq!(remaining["assets"].as_array().unwrap().len(), 3);
    request.inputs = serde_json::from_value::<Vec<Cell>>(remaining["cells"].clone())
        .unwrap()
        .iter()
        .map(|c| c.out_point.clone())
        .collect();
    request.amounts = None;
    let quote2 = call("prepare", Some(json!(request)), true);
    assert!(quote2.get("error").is_none(), "{quote2}");
    let q: claim::Quote = serde_json::from_value(quote2).unwrap();
    let result = call(
        "submit",
        Some(
            json!({"request":request,"transaction_hash":q.transaction_hash,"signing_message":q.signing_message,"signature":signature(&q.signing_message,1)}),
        ),
        true,
    );
    assert_eq!(result["status"], "submitted", "{result}");
    assert!(
        call(&format!("account/{}", request.address), None, true)["cells"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    println!("Manual deposit wallet: exact fee {size} shannons, signed partial and remainder transfers executed by CKB-VM");
}

fn partial_amounts(assets: &[Asset], inputs: &[Cell]) -> Vec<claim::ClaimAmount> {
    let hash = inputs[1]
        .output
        .type_script
        .as_ref()
        .unwrap()
        .hash()
        .unwrap();
    vec![
        claim::ClaimAmount {
            asset_id: 1,
            raw: (600 * CAP).to_string(),
        },
        claim::ClaimAmount {
            asset_id: assets
                .iter()
                .find(|a| a.type_hash == hash)
                .unwrap()
                .asset_id,
            raw: "50000000".into(),
        },
    ]
}

#[test]
fn claim_portal_partial_then_remainder_and_second_overdraw_rejected() {
    let (mut node, deps, inputs, assets, mut r) = setup_claim();
    r.amounts = Some(partial_amounts(&assets, &inputs));
    let q = claim::construct(&r, inputs.clone(), &assets, &deps, 1000).unwrap();
    let signed = claim::assemble(&q, &signature(&q.signing_message, 1)).unwrap();
    let cycles = node.vm(&signed).unwrap();
    println!("Partial OmniLock withdrawal: {cycles} cycles");
    assert!(cycles < claim::MAX_CYCLES);
    let owner = claim::owner(&r.address).unwrap();
    let recipient = claim::parse_address(&r.destination).unwrap();
    let remaining = signed
        .outputs
        .iter()
        .enumerate()
        .filter(|(_, o)| o.lock == owner)
        .map(|(i, _)| signed.cell(&point(&signed, i)).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(remaining.len(), 2);
    assert_eq!(q.received_capacity_shannons, (600 * CAP).to_string());
    assert_eq!(
        remaining
            .iter()
            .map(|c| quantity(&c.output.capacity).unwrap())
            .sum::<u64>(),
        800 * CAP - q.fee_shannons.parse::<u64>().unwrap()
    );
    let first_type = inputs[1].output.type_script.as_ref().unwrap();
    let amount_of = |cells: &[Cell], ty: &Script| {
        cells
            .iter()
            .filter(|c| c.output.type_script.as_ref() == Some(ty))
            .map(|c| u128::from_le_bytes(bytes(&c.output_data).unwrap().try_into().unwrap()))
            .sum::<u128>()
    };
    assert_eq!(amount_of(&remaining, first_type), 73456789);
    assert_eq!(
        amount_of(&remaining, inputs[2].output.type_script.as_ref().unwrap()),
        987654321
    );
    for (o, d) in signed.outputs.iter().zip(&signed.outputs_data) {
        assert!(o.lock == owner || o.lock == recipient);
        assert!(quantity(&o.capacity).unwrap() >= claim::occupied(o, d).unwrap());
    }
    node.insert(signed.clone());
    let mut second = r.clone();
    second.inputs = remaining.iter().map(|c| c.out_point.clone()).collect();
    let mut excessive = second.clone();
    excessive.amounts.as_mut().unwrap()[1].raw = "73456790".into();
    assert!(
        claim::construct(&excessive, remaining.clone(), &assets, &deps, 1000)
            .unwrap_err()
            .to_string()
            .contains("exceeds remaining balance")
    );
    excessive.amounts.as_mut().unwrap()[1].raw = "1".into();
    excessive.amounts.as_mut().unwrap()[0].raw = (800 * CAP).to_string();
    assert!(claim::construct(&excessive, remaining.clone(), &assets, &deps, 1000).is_err());
    assert!(node.vm(&signed).is_err()); // Spent inputs cannot be reused.
    second.amounts = None;
    let q2 = claim::construct(&second, remaining.clone(), &assets, &deps, 1000).unwrap();
    let signed2 = claim::assemble(&q2, &signature(&q2.signing_message, 1)).unwrap();
    let cycles2 = node.vm(&signed2).unwrap();
    println!("Remaining OmniLock withdrawal: {cycles2} cycles");
    assert!(cycles2 < claim::MAX_CYCLES);
    assert_eq!(
        q2.received_capacity_shannons.parse::<u64>().unwrap()
            + q.fee_shannons.parse::<u64>().unwrap()
            + q2.fee_shannons.parse::<u64>().unwrap(),
        800 * CAP
    );
    let outputs2 = signed2
        .outputs
        .iter()
        .enumerate()
        .map(|(i, _)| signed2.cell(&point(&signed2, i)).unwrap())
        .collect::<Vec<_>>();
    assert!(outputs2.iter().all(|c| c.output.lock == recipient));
    assert_eq!(amount_of(&outputs2, first_type), 73456789);
    assert_eq!(
        amount_of(&outputs2, inputs[2].output.type_script.as_ref().unwrap()),
        987654321
    );
    // Even a freshly signed attempt to mint one unit is rejected by the actual sUDT VM.
    let mut forged = q2.clone();
    let i = forged
        .transaction
        .outputs
        .iter()
        .position(|o| o.type_script.as_ref() == Some(first_type))
        .unwrap();
    forged.transaction.outputs_data[i] = hx(73456790u128.to_le_bytes());
    forged.signing_message = hx(claim::signing_message(&forged.transaction).unwrap());
    forged.transaction_hash = forged.transaction.hash().unwrap();
    let result =
        node.vm(&claim::assemble(&forged, &signature(&forged.signing_message, 1)).unwrap());
    assert!(result.is_err());
    let rejection = result.unwrap_err().to_string();
    // Require a type-script validation failure, not a lock/signature or input-resolution failure.
    assert!(
        rejection.contains("Type")
            && rejection.contains("ValidationFailure")
            && rejection.contains(claim::SUDT.trim_start_matches("0x"))
            && rejection.contains("error code -52"),
        "Unexpected rejection: {rejection}"
    );
    println!("Freshly signed second-withdrawal type-script rejection: {rejection}");
    if let Some(path) = std::env::var_os("GW_PARTIAL_CLAIM_FIXTURE") {
        std::fs::write(path, serde_json::to_vec_pretty(&json!({
            "request": r, "quote": q, "signature": signature(&q.signing_message, 1),
            "remaining_cells": remaining, "second_request": second, "second_quote": q2,
            "second_signature": signature(&q2.signing_message, 1),
            "config": {"network":"CKB mainnet","scope":"Godwoken v1","assets":assets,"dependencies":deps,"broadcast_enabled":true,"fee_rate":1000,"plan_manifest":hx([0;32]),"snapshot_root":hx([0;32])}
        })).unwrap()).unwrap();
    }
    node.insert(signed2);
    assert!(!node.live.values().any(|c| c.output.lock == owner));
}

#[test]
fn claim_portal_partial_invalid_amounts_and_change_capacity() {
    let (_, deps, inputs, assets, mut r) = setup_claim();
    r.amounts = Some(partial_amounts(&assets, &inputs));
    let check = |r: &BuildRequest| {
        assert!(claim::construct(r, inputs.clone(), &assets, &deps, 1000).is_err())
    };
    let mut bad = r.clone();
    bad.amounts = Some(vec![]);
    check(&bad);
    let mut bad = r.clone();
    bad.amounts.as_mut().unwrap().remove(0);
    check(&bad);
    for value in [
        "-1",
        "1.5",
        "+1",
        "",
        "340282366920938463463374607431768211456",
    ] {
        let mut bad = r.clone();
        bad.amounts.as_mut().unwrap()[1].raw = value.into();
        check(&bad);
    }
    let mut bad = r.clone();
    bad.amounts.as_mut().unwrap()[1].asset_id = 999999;
    check(&bad);
    let mut bad = r.clone();
    let duplicate = bad.amounts.as_ref().unwrap()[0].clone();
    bad.amounts.as_mut().unwrap().push(duplicate);
    check(&bad);
    let mut bad = r.clone();
    bad.amounts.as_mut().unwrap()[0].raw = "1".into();
    check(&bad);
    let mut bad = r.clone();
    bad.amounts.as_mut().unwrap()[0].raw = (1399 * CAP).to_string();
    check(&bad);
}

#[test]
fn claim_portal_partial_plain_ckb_and_zero_token_change() {
    let (mut node, deps, inputs, assets, mut r) = setup_claim();
    let owner = claim::owner(&r.address).unwrap();
    r.inputs = vec![inputs[0].out_point.clone()];
    r.amounts = Some(vec![claim::ClaimAmount {
        asset_id: 1,
        raw: (600 * CAP).to_string(),
    }]);
    let q = claim::construct(&r, vec![inputs[0].clone()], &assets, &deps, 1000).unwrap();
    assert_eq!(q.transaction.outputs.len(), 2);
    assert_eq!(q.transaction.outputs[1].lock, owner);
    assert!(q.transaction.outputs[1].type_script.is_none());
    assert_eq!(
        quantity(&q.transaction.outputs[1].capacity).unwrap(),
        400 * CAP - q.fee_shannons.parse::<u64>().unwrap()
    );
    assert!(node
        .vm(&claim::assemble(&q, &signature(&q.signing_message, 1)).unwrap())
        .is_ok());
    let mut outputs = inputs.iter().map(|c| c.output.clone()).collect::<Vec<_>>();
    outputs.pop();
    let funding = base_tx(outputs, vec!["0x".into(), hx(0u128.to_le_bytes())]);
    let cells = (0..2)
        .map(|i| funding.cell(&point(&funding, i)).unwrap())
        .collect::<Vec<_>>();
    node.insert(funding);
    r.inputs = cells.iter().map(|c| c.out_point.clone()).collect();
    let q = claim::construct(&r, cells, &assets, &deps, 1000).unwrap();
    assert_eq!(q.transaction.outputs[1].lock, owner);
    assert!(q.transaction.outputs[1].type_script.is_some());
    assert_eq!(q.transaction.outputs_data[1], hx(0u128.to_le_bytes()));
    assert!(node
        .vm(&claim::assemble(&q, &signature(&q.signing_message, 1)).unwrap())
        .is_ok());
}

#[path = "v0_identity_e2e.rs"]
mod v0_identity;

#[path = "v0_upgrade_e2e.rs"]
mod v0_upgrade;

#[path = "v0_settlement_e2e.rs"]
mod v0_settlement;
