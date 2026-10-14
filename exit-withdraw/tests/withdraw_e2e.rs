#![cfg(feature = "e2e-tests")]
#[allow(dead_code)]
mod support;
use anyhow::{Context as _, Result};
use ckb_testtool::{
    ckb_types::{
        bytes::Bytes, core::TransactionView, packed as c, prelude::Entity as CEntity,
        prelude::IntoTransactionView,
    },
    context::Context,
};
use exit_withdraw_state_validator as contract;
use godwoken_withdraw::{
    db::Database,
    model::*,
    withdraw::{
        self,
        build::{Request, Skeleton},
        chain::Chain,
        Dependency, Deployment, Transaction,
    },
};
use gw_types::{packed, prelude::*};
use secp256k1::{Message, PublicKey, Secp256k1, SecretKey};
use serde_json::{json, Value};
use sha3::{Digest, Keccak256};
use std::{
    collections::BTreeMap,
    io::{Read, Write},
    net::TcpListener,
    path::Path,
    process::Command,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};
const CAP: u64 = 100_000_000;
fn key(n: u8) -> SecretKey {
    let mut k = [0; 32];
    k[31] = n;
    SecretKey::from_slice(&k).unwrap()
}
fn eth(n: u8) -> [u8; 20] {
    let pk = PublicKey::from_secret_key(&Secp256k1::new(), &key(n)).serialize_uncompressed();
    Keccak256::digest(&pk[1..])[12..].try_into().unwrap()
}
fn multisig(n: u8) -> Vec<u8> {
    let pk = PublicKey::from_secret_key(&Secp256k1::new(), &key(n)).serialize();
    [&[0, 0, 1, 1][..], &hash(&pk)[..20]].concat()
}
fn script(code: [u8; 32], args: Vec<u8>) -> Script {
    Script {
        code_hash: hx(code),
        hash_type: "type".into(),
        args: hx(args),
    }
}
fn omni_signature(signature: &[u8]) -> Vec<u8> {
    let total = 20 + signature.len();
    let mut b = Vec::new();
    for n in [total, 16, total, total, signature.len()] {
        b.extend_from_slice(&(n as u32).to_le_bytes());
    }
    b.extend_from_slice(signature);
    b
}
fn base_tx(outputs: Vec<CellOutput>, data: Vec<String>) -> Transaction {
    Transaction {
        version: "0x0".into(),
        cell_deps: vec![],
        header_deps: vec![],
        inputs: vec![],
        outputs,
        outputs_data: data,
        witnesses: vec![],
    }
}
fn point(tx: &Transaction, index: usize) -> OutPoint {
    OutPoint {
        tx_hash: tx.hash().unwrap(),
        index: format!("0x{index:x}"),
    }
}
#[derive(Clone, Default)]
struct Node {
    txs: BTreeMap<String, Transaction>,
    live: BTreeMap<OutPoint, Cell>,
    orphan: bool,
    unconfirmed: bool,
}
impl Node {
    fn insert(&mut self, tx: Transaction) {
        let hash = tx.hash().unwrap();
        for p in tx.input_points().unwrap() {
            self.live.remove(&p);
        }
        for i in 0..tx.outputs.len() {
            let p = point(&tx, i);
            self.live.insert(p.clone(), tx.cell(&p).unwrap());
        }
        self.txs.insert(hash, tx);
    }
    fn vm(&self, tx: &Transaction) -> Result<u64> {
        let mut ctx = Context::default();
        for p in tx.input_points()?.iter().chain(
            tx.cell_deps
                .iter()
                .map(|d| serde_json::from_value::<OutPoint>(d["out_point"].clone()).unwrap())
                .collect::<Vec<_>>()
                .iter(),
        ) {
            let cell = self.live.get(p).context("VM missing live cell")?;
            ctx.cells.insert(
                c::OutPoint::from_slice(withdraw::outpoint(p)?.as_slice())?,
                (
                    c::CellOutput::from_slice(cell.output.packed()?.as_slice())?,
                    Bytes::from(bytes(&cell.output_data)?),
                ),
            );
        }
        let packed = tx.packed()?;
        let tx: TransactionView = c::Transaction::from_slice(packed.as_slice())?.into_view();
        ctx.verify_tx(&tx, 70_000_000)
            .map_err(|e| anyhow::anyhow!("Full VM: {e:?}"))
    }
}
impl Chain for Node {
    fn call(&self, method: &str, params: Value) -> Result<Value> {
        let block = hx([42; 32]);
        let header = json!({"hash":block,"number":"0x64"});
        Ok(match method {
            "get_tip_header" => json!({"hash":hx([43;32]),"number":"0x80"}),
            "get_indexer_tip" => json!({"block_hash":hx([43;32]),"block_number":"0x80"}),
            "get_header" => header,
            "get_header_by_number" => {
                if params[0] == "0x0" {
                    json!({"hash":godwoken_withdraw::claim::GENESIS,"number":"0x0"})
                } else if params[0] == "0x80" {
                    json!({"hash":hx([43;32]),"number":"0x80"})
                } else if self.orphan {
                    json!({"hash":hx([44;32]),"number":"0x64"})
                } else {
                    header
                }
            }
            "get_transaction" => {
                let tx = self
                    .txs
                    .get(params[0].as_str().unwrap())
                    .context("Pruned history")?;
                json!({"transaction":tx,"tx_status":{"status":if self.unconfirmed{"pending"}else{"committed"},"block_hash":block}})
            }
            "get_live_cell" => {
                let p: OutPoint = serde_json::from_value(params[0].clone())?;
                match self.live.get(&p) {
                    Some(c) => {
                        json!({"status":"live","cell":{"output":c.output,"data":{"content":c.output_data}}})
                    }
                    None => json!({"status":"dead"}),
                }
            }
            "get_cells" => {
                let requested: Script = serde_json::from_value(params[0]["script"].clone())?;
                if params.as_array().unwrap().len() > 3 {
                    json!({"objects":[],"last_cursor":"end"})
                } else {
                    let mut matches = vec![];
                    for c in self.live.values() {
                        let actual = if params[0]["script_type"] == "lock" {
                            Some(&c.output.lock)
                        } else {
                            c.output.type_script.as_ref()
                        };
                        if let Some(a) = actual {
                            if a.code_hash == requested.code_hash
                                && a.hash_type == requested.hash_type
                                && (if params[0]["script_search_mode"] == "prefix" {
                                    a.args.starts_with(&requested.args)
                                } else {
                                    a.args == requested.args
                                })
                            {
                                let mut c = c.clone();
                                c.block_number = "0x64".into();
                                c.tx_index = "0x0".into();
                                matches.push(c);
                            }
                        }
                    }
                    json!({"objects":matches,"last_cursor":"end"})
                }
            }
            "estimate_cycles" => {
                json!({"cycles":format!("0x{:x}",self.vm(&serde_json::from_value(params[0].clone())?)?)})
            }
            _ => anyhow::bail!("Forbidden/unexpected method {method}"),
        })
    }
}
struct Setup {
    d: Deployment,
    b: Bundle,
    node: Node,
}
fn setup(dbpath: &Path) -> Setup {
    let mut b = support::fixture_address(dbpath, 123, 500, eth(1));
    let gov = script(contract::MULTISIG, hash(&multisig(3))[..20].to_vec());
    let old: Value = serde_json::from_str(include_str!(
        "fixtures/mainnet-evidence/eth-lock-research/godwoken-omnilock-live.json"
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
        [&[1][..], &eth(2), &[0]].concat(),
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
    let p = contract::Profile {
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
        "fixtures/mainnet-evidence/v1-code-live-state_validator.json"
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
        "fixtures/mainnet-evidence/v1-code-live-custodian_lock.json"
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
        "fixtures/mainnet-evidence/eth-lock-research/standard-omnilock-live.json"
    ))
    .unwrap();
    let c = &standard["result"]["cell"];
    deploy(
        Some(serde_json::from_value(c["output"]["type"].clone()).unwrap()),
        bytes(c["data"]["content"].as_str().unwrap()).unwrap(),
    );
    let sudt: Value =
        serde_json::from_str(include_str!("fixtures/mainnet-sudt-live.json")).unwrap();
    let c = &sudt["result"]["cell"];
    let typ: Script = serde_json::from_value(c["output"]["type"].clone()).unwrap();
    assert_eq!(typ.hash().unwrap(), cfg.hashes["l1_sudt"]);
    deploy(
        Some(typ),
        bytes(c["data"]["content"].as_str().unwrap()).unwrap(),
    );
    let m: Value =
        serde_json::from_str(include_str!("fixtures/mainnet-multisig-type.json")).unwrap();
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
fn sign_group(tx: &mut Transaction, indices: &[usize], secret: u8, omni: bool) {
    sign_group_with_mode(tx,indices,secret,omni,omni)
}
fn sign_group_with_mode(tx: &mut Transaction, indices: &[usize], secret: u8, omni: bool, ethereum: bool) {
    let mut hasher = gw_hash::blake2b::new_blake2b();
    hasher.update(&hash32(&tx.hash().unwrap()).unwrap());
    for &i in indices {
        let mut w = bytes(&tx.witnesses[i]).unwrap();
        if omni && i == indices[0] {
            let args = packed::WitnessArgs::from_slice(&w).unwrap();
            let len = args.lock().to_opt().unwrap().raw_data().len();
            w = args
                .as_builder()
                .lock(Some(gw_types::bytes::Bytes::from(vec![0; len])).pack())
                .build()
                .as_slice()
                .to_vec();
        }
        hasher.update(&(w.len() as u64).to_le_bytes());
        hasher.update(&w);
    }
    for w in &tx.witnesses[tx.inputs.len()..] {
        let b = bytes(w).unwrap();
        hasher.update(&(b.len() as u64).to_le_bytes());
        hasher.update(&b);
    }
    let mut digest = [0; 32];
    hasher.finalize(&mut digest);
    if ethereum {
        digest =
            Keccak256::digest([b"\x19Ethereum Signed Message:\n32".as_slice(), &digest].concat())
                .into();
    }
    let sig = Secp256k1::new().sign_ecdsa_recoverable(&Message::from_digest(digest), &key(secret));
    let (recid, compact) = sig.serialize_compact();
    let signature = [compact.as_slice(), &[i32::from(recid) as u8]].concat();
    let w = packed::WitnessArgs::from_slice(&bytes(&tx.witnesses[indices[0]]).unwrap()).unwrap();
    let lock = if omni {
        omni_signature(&signature)
    } else {
        [multisig(secret).as_slice(), &signature].concat()
    };
    tx.witnesses[indices[0]] = hx(w
        .as_builder()
        .lock(Some(gw_types::bytes::Bytes::from(lock)).pack())
        .build()
        .as_slice());
}
fn sign(s: &Skeleton) -> Transaction {
    let mut tx = s.transaction.clone();
    for group in s.signing_groups.as_array().unwrap() {
        let indices: Vec<_> = group["input_indices"]
            .as_array()
            .unwrap()
            .iter()
            .map(|n| n.as_u64().unwrap() as usize)
            .collect();
        let omni = group["kind"] == "bootstrap_lock";
        sign_group(&mut tx, &indices, if omni { 2 } else { 3 }, omni);
    }
    tx
}
#[test]
fn export_withdraw_profile() {
    let t = tempfile::tempdir().unwrap();
    let s = setup(&t.path().join("db"));
    if let Ok(path) = std::env::var("GW_WITHDRAW_TEST_PROFILE") {
        std::fs::write(
            format!("{path}.hash"),
            s.d.profile_hash.trim_start_matches("0x"),
        )
        .unwrap();
        std::fs::write(path, bytes(&s.d.profile).unwrap()).unwrap();
    }
}

fn request() -> Vec<Request> {
    serde_json::from_value(
        json!([{"eth_address":hx(eth(1)),"amounts":{"1":"500000000000000000000","3":"100"}}]),
    )
    .unwrap()
}
fn advance(s: &mut Setup, tx: Transaction) {
    s.b.rollup_cell = tx.cell(&point(&tx, 0)).unwrap();
    let change = Script::from_packed(s.d.profile().unwrap().change).unwrap();
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
            .filter(|(_, o)| o.lock == change)
            .map(|(i, _)| tx.cell(&point(&tx, i)).unwrap()),
    );
    s.b.cells.insert("custodian".into(), custodians);
    s.d.sponsor_inputs = vec![point(&tx, tx.outputs.len() - 1)];
    s.node.insert(tx);
}
#[test]
fn withdrawal_restart_replay_and_user_claim() {
    assert!(
        std::env::var_os("GW_WITHDRAW_E2E_BINARY").is_some(),
        "Run scripts/test-withdraw-e2e.sh"
    );
    let t = tempfile::tempdir().unwrap();
    let path = t.path().join("db");
    let mut s = setup(&path);
    let original = Database::open(&path).unwrap().tip.account_root;
    let first = {
        let db = Database::open(&path).unwrap();
        withdraw::prepare(&db, &s.node, &s.d, &s.b, &request()).unwrap()
    };
    assert_eq!(first.journal.as_array().unwrap().len(), 0);
    let signed = sign(&first);
    let cycles = s.node.vm(&signed).unwrap();
    println!("bootstrap full transaction cycles: {cycles}");
    let db = Database::open(&path).unwrap();
    let verified = withdraw::verify_signed(&db, &s.node, &s.d, &s.b, &signed, 70_000_000).unwrap();
    assert_eq!(verified["cycles"], cycles);
    assert!(withdraw::verify_signed(&db, &s.node, &s.d, &s.b, &signed, 1).is_err());
    let mut bad = signed.clone();
    bad.outputs[1].lock.args = hx([&[1][..], &eth(4), &[0]].concat());
    assert!(withdraw::verify_signed(&db, &s.node, &s.d, &s.b, &bad, 70_000_000).is_err());
    let mut bad = signed.clone();
    bad.outputs_data[1] = hx(101u128.to_le_bytes());
    assert!(withdraw::verify_signed(&db, &s.node, &s.d, &s.b, &bad, 70_000_000).is_err());
    assert!(s.node.vm(&first.transaction).is_err());
    drop(db);
    advance(&mut s, signed.clone());
    // No local journal or mutated DB is supplied on restart.
    let db = Database::open(&path).unwrap();
    assert_eq!(db.tip.account_root, original);
    let second = withdraw::prepare(&db, &s.node, &s.d, &s.b, &request()).unwrap();
    assert_eq!(second.before_root, first.after_root);
    assert_ne!(second.before_root, second.after_root);
    assert_eq!(second.journal.as_array().unwrap().len(), 1);
    // Witnesses are not covered by the raw transaction hash: replay must validate them.
    let first_hash = signed.hash().unwrap();
    let saved = s.node.txs[&first_hash].clone();
    let mut corrupt = saved.clone();
    let w = packed::WitnessArgs::from_slice(&bytes(&corrupt.witnesses[0]).unwrap()).unwrap();
    let mut a = withdraw::chain::action(&corrupt, &s.d.profile().unwrap()).unwrap();
    a.kv[0].1[0] ^= 1;
    corrupt.witnesses[0] = hx(w
        .as_builder()
        .output_type(
            Some(gw_types::bytes::Bytes::from(
                contract::codec::encode_action(&a),
            ))
            .pack(),
        )
        .build()
        .as_slice());
    s.node.txs.insert(first_hash.clone(), corrupt);
    assert!(withdraw::prepare(&db, &s.node, &s.d, &s.b, &request()).is_err());
    s.node.txs.remove(&first_hash);
    assert!(withdraw::prepare(&db, &s.node, &s.d, &s.b, &request()).is_err());
    s.node.txs.insert(first_hash, saved);
    let mut bad_deployment = s.d.clone();
    bad_deployment.fee_shannons += 1;
    let signed2 = sign(&second);
    assert!(
        withdraw::verify_signed(&db, &s.node, &bad_deployment, &s.b, &signed2, 70_000_000).is_err()
    );
    let cycles = s.node.vm(&signed2).unwrap();
    println!("continuation full transaction cycles: {cycles}");
    withdraw::verify_signed(&db, &s.node, &s.d, &s.b, &signed2, 70_000_000).unwrap();
    assert!(withdraw::verify_signed(&db, &s.node, &s.d, &s.b, &signed, 70_000_000).is_err());
    // User claims the first payout with an Ethereum personal_sign signature.
    let mut claim = base_tx(
        vec![signed.outputs[1].clone()],
        vec![signed.outputs_data[1].clone()],
    );
    claim.outputs[0].lock = Script::from_packed(s.d.profile().unwrap().governance).unwrap();
    claim.outputs[0].capacity = format!(
        "0x{:x}",
        quantity(&claim.outputs[0].capacity).unwrap() - 100_000
    );
    claim.inputs = vec![json!({"since":"0x0","previous_output":point(&signed,1)})];
    claim.cell_deps = signed.cell_deps.clone();
    claim.witnesses = vec![hx(packed::WitnessArgs::new_builder()
        .lock(Some(gw_types::bytes::Bytes::from(omni_signature(&[0; 65]))).pack())
        .build()
        .as_slice())];
    let mut wrong = claim.clone();
    sign_group(&mut wrong, &[0], 4, true);
    assert!(s.node.vm(&wrong).is_err());
    sign_group(&mut claim, &[0], 1, true);
    println!(
        "user claim full transaction cycles: {}",
        s.node.vm(&claim).unwrap()
    );
    advance(&mut s, signed2);
    let third =
        withdraw::prepare(&db, &s.node, &s.d, &s.b, &[Request::Address(hx(eth(1)))]).unwrap();
    assert_eq!(third.journal.as_array().unwrap().len(), 2);
    let final_tx = sign(&third);
    println!(
        "final withdrawal full transaction cycles: {}",
        s.node.vm(&final_tx).unwrap()
    );
    advance(&mut s, final_tx);
    assert!(withdraw::prepare(&db, &s.node, &s.d, &s.b, &request()).is_err());
}
#[test]
fn reconstruction_and_request_fail_closed() {
    let t = tempfile::tempdir().unwrap();
    let path = t.path().join("db");
    let mut s = setup(&path);
    let db = Database::open(&path).unwrap();
    let mut duplicate = request();
    duplicate.extend(request());
    assert!(withdraw::prepare(&db, &s.node, &s.d, &s.b, &duplicate)
        .unwrap_err()
        .to_string()
        .contains("Duplicate"));
    let over: Vec<Request> =
        serde_json::from_value(json!([{"eth_address":hx(eth(1)),"amounts":{"3":"501"}}])).unwrap();
    assert!(withdraw::prepare(&db, &s.node, &s.d, &s.b, &over).is_err());
    assert!(withdraw::prepare(&db, &s.node, &s.d, &s.b, &[Request::Address(hx(eth(4)))]).is_err());
    s.node.orphan = true;
    assert!(withdraw::prepare(&db, &s.node, &s.d, &s.b, &request()).is_err());
    s.node.orphan = false;
    s.node.unconfirmed = true;
    assert!(withdraw::prepare(&db, &s.node, &s.d, &s.b, &request()).is_err());
    s.node.unconfirmed = false;
    s.d.confirmations = 100;
    assert!(withdraw::prepare(&db, &s.node, &s.d, &s.b, &request()).is_err());
    s.d.confirmations = 10;
    s.node.txs.remove(&s.b.rollup_cell.out_point.tx_hash);
    assert!(withdraw::prepare(&db, &s.node, &s.d, &s.b, &request()).is_err());
}

struct Server {
    url: String,
    node: Arc<Mutex<Node>>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Server {
    fn new(node: Node) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        listener.set_nonblocking(true).unwrap();
        let node = Arc::new(Mutex::new(node));
        let stop = Arc::new(AtomicBool::new(false));
        let n = node.clone();
        let st = stop.clone();
        let thread = std::thread::spawn(move || {
            while !st.load(Ordering::Relaxed) {
                let (mut stream, _) = match listener.accept() {
                    Ok(s) => s,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(std::time::Duration::from_millis(1));
                        continue;
                    }
                    Err(e) => panic!("{e}"),
                };
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(std::time::Duration::from_secs(10)))
                    .unwrap();
                let mut data = Vec::new();
                let mut buf = [0; 8192];
                let header_end = loop {
                    let count = stream.read(&mut buf).unwrap();
                    assert!(count > 0);
                    data.extend_from_slice(&buf[..count]);
                    if let Some(i) = data.windows(4).position(|x| x == b"\r\n\r\n") {
                        break i + 4;
                    }
                };
                let length = String::from_utf8_lossy(&data[..header_end])
                    .lines()
                    .find_map(|l| {
                        l.to_ascii_lowercase()
                            .strip_prefix("content-length:")
                            .map(|s| s.trim().parse::<usize>().unwrap())
                    })
                    .unwrap();
                while data.len() < header_end + length {
                    let count = stream.read(&mut buf).unwrap();
                    assert!(count > 0);
                    data.extend_from_slice(&buf[..count]);
                }
                let req: Value =
                    serde_json::from_slice(&data[header_end..header_end + length]).unwrap();
                let result = {
                    let mut node = n.lock().unwrap();
                    if req["method"] == "send_transaction" {
                        (|| -> Result<Value> {
                            let tx: Transaction = serde_json::from_value(req["params"][0].clone())?;
                            node.vm(&tx)?;
                            let hash = tx.hash()?;
                            node.insert(tx);
                            Ok(json!(hash))
                        })()
                    } else {
                        node.call(req["method"].as_str().unwrap(), req["params"].clone())
                    }
                };
                let response=match result{Ok(v)=>json!({"jsonrpc":"2.0","id":req["id"],"result":v}),Err(e)=>json!({"jsonrpc":"2.0","id":req["id"],"error":{"code":-1,"message":format!("{e:#}")}})}.to_string();
                write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",response.len(),response).unwrap();
            }
        });
        Self {
            url,
            node,
            stop,
            thread: Some(thread),
        }
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = self.thread.take().unwrap().join();
    }
}
#[test]
fn native_only_preserves_zero_token_custodian() {
    assert!(
        std::env::var_os("GW_WITHDRAW_E2E_BINARY").is_some(),
        "Run scripts/test-withdraw-e2e.sh"
    );
    let t = tempfile::tempdir().unwrap();
    let path = t.path().join("db");
    let mut s = setup(&path);
    let db = Database::open(&path).unwrap();
    let mut output = s.b.cells["custodian"][1].output.clone();
    output.capacity = format!("0x{:x}", 2000 * CAP);
    let funding = base_tx(vec![output], vec![hx(0u128.to_le_bytes())]);
    let cell = funding.cell(&point(&funding, 0)).unwrap();
    s.node.insert(funding);
    s.b.cells.insert("custodian".into(), vec![cell]);
    let requests: Vec<Request> = serde_json::from_value(
        json!([{"eth_address":hx(eth(1)),"amounts":{"1":"500000000000000000000"}}]),
    )
    .unwrap();
    let sk = withdraw::prepare(&db, &s.node, &s.d, &s.b, &requests).unwrap();
    assert!(sk
        .transaction
        .outputs
        .iter()
        .zip(&sk.transaction.outputs_data)
        .any(|(o, d)| o.type_script.as_ref()
            == s.b.cells["custodian"][0].output.type_script.as_ref()
            && *d == hx(0u128.to_le_bytes())));
    s.node.vm(&sign(&sk)).unwrap();
}

#[test]
fn malformed_requests_dependencies_and_root_rejected() {
    let t = tempfile::tempdir().unwrap();
    let path = t.path().join("db");
    let mut s = setup(&path);
    let db = Database::open(&path).unwrap();
    for input in [
        json!([{"eth_address":hx(eth(1)),"amounts":{"01":"1"}}]),
        json!([{"eth_address":hx(eth(1)),"amounts":{},"ammount":"1"}]),
    ] {
        assert!(serde_json::from_value::<Vec<Request>>(input).is_err());
    }
    let bad: Vec<Request> =
        serde_json::from_value(json!([{"eth_address":hx(eth(1)),"amounts":{"999":"1"}}])).unwrap();
    assert!(withdraw::prepare(&db, &s.node, &s.d, &s.b, &bad).is_err());
    let mut wrong = s.b.clone();
    let gs =
        packed::GlobalState::from_slice(&bytes(&wrong.rollup_cell.output_data).unwrap()).unwrap();
    wrong.rollup_cell.output_data = hx(gs
        .clone()
        .as_builder()
        .account(
            gs.account()
                .as_builder()
                .merkle_root([9; 32].pack())
                .build(),
        )
        .build()
        .as_slice());
    assert!(withdraw::prepare(&db, &s.node, &s.d, &wrong, &request()).is_err());
    let mut dep = s.d.clone();
    dep.cell_deps[0].data_hash = hx([9; 32]);
    assert!(withdraw::prepare(&db, &s.node, &dep, &s.b, &request()).is_err());
    let mut dep = s.d.clone();
    dep.cell_deps.retain(|d| {
        s.node.live[&d.out_point]
            .output
            .type_script
            .as_ref()
            .and_then(|t| t.hash().ok())
            != Some(hx(contract::MULTISIG))
    });
    assert!(withdraw::prepare(&db, &s.node, &dep, &s.b, &request()).is_err());
    s.node.live.remove(&s.d.sponsor_inputs[0]);
    assert!(withdraw::prepare(&db, &s.node, &s.d, &s.b, &request()).is_err());
}

/// Complete both native and token exits, including committing the user's claim
/// and spending the received cell with an independent CKB wallet key.
#[test]
fn simulated_chain_exit_to_omnilock_to_user_ckb_wallet() {
    assert!(
        std::env::var_os("GW_WITHDRAW_E2E_BINARY").is_some(),
        "Run scripts/test-withdraw-e2e.sh"
    );
    for with_token in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("db");
        let mut s = setup(&path);
        let db = Database::open(&path).unwrap();
        let requests = if with_token {
            request()
        } else {
            serde_json::from_value(
                json!([{"eth_address":hx(eth(1)),"amounts":{"1":"500000000000000000000"}}]),
            )
            .unwrap()
        };
        let profile = s.d.profile().unwrap();
        let cfg = Config::parse(&bytes(&s.b.config_data).unwrap()).unwrap();
        let read_state = |s: &Setup| {
            let (state, _) = withdraw::chain::reconstruct(
                &db,
                &s.node,
                &profile,
                &cfg.packed,
                &s.b.rollup_cell,
                10,
            )
            .unwrap();
            [
                contract::balance_key(1, eth(1)),
                contract::balance_key(3, eth(1)),
                contract::supply_key(1),
                contract::supply_key(3),
            ]
            .map(|key| contract::amount(state.read(key).unwrap()))
        };
        let before = read_state(&s);
        let pool_capacity = |s: &Setup| {
            s.b.cells["custodian"]
                .iter()
                .map(|c| quantity(&c.output.capacity).unwrap())
                .sum::<u64>()
        };
        let pool_tokens = |s: &Setup| {
            s.b.cells["custodian"]
                .iter()
                .filter(|c| c.output.type_script.is_some())
                .map(|c| u128::from_le_bytes(bytes(&c.output_data).unwrap().try_into().unwrap()))
                .sum::<u128>()
        };
        let original_capacity = pool_capacity(&s);
        let original_tokens = pool_tokens(&s);
        let original_rollup = s.b.rollup_cell.out_point.clone();
        let skeleton = withdraw::prepare(&db, &s.node, &s.d, &s.b, &requests).unwrap();
        let exit = sign(&skeleton);
        let exit_cycles = s.node.vm(&exit).unwrap();
        advance(&mut s, exit.clone());
        assert!(!s.node.live.contains_key(&original_rollup));
        assert_eq!(
            GlobalState::parse(&bytes(&s.b.rollup_cell.output_data).unwrap())
                .unwrap()
                .account_root,
            skeleton.after_root
        );
        let after = read_state(&s);
        let native_debit = gw_types::U256::from(500u64) * gw_types::U256::exp10(18);
        let token_debit = gw_types::U256::from(if with_token { 100u64 } else { 0 });
        assert_eq!(before[0] - after[0], native_debit);
        assert_eq!(before[2] - after[2], native_debit);
        assert_eq!(before[1] - after[1], token_debit);
        assert_eq!(before[3] - after[3], token_debit);
        assert_eq!(original_capacity - pool_capacity(&s), 500 * CAP);
        assert_eq!(
            original_tokens - pool_tokens(&s),
            if with_token { 100 } else { 0 }
        );
        let payout = point(&exit, 1);
        let payout_cell = s.node.live[&payout].clone();
        assert_eq!(
            payout_cell.output.lock.packed().unwrap(),
            contract::eth_lock(eth(1))
        );
        assert_eq!(quantity(&payout_cell.output.capacity).unwrap(), 500 * CAP);
        assert_eq!(payout_cell.output.type_script.is_some(), with_token);
        if with_token {
            assert_eq!(payout_cell.output_data, hx(100u128.to_le_bytes()));
        }

        // An independent user wallet; neither the governance key nor bootstrap key.
        let wallet = script(contract::MULTISIG, hash(&multisig(5))[..20].to_vec());
        let mut claim = base_tx(
            vec![payout_cell.output.clone()],
            vec![payout_cell.output_data.clone()],
        );
        claim.outputs[0].lock = wallet.clone();
        claim.outputs[0].capacity = format!("0x{:x}", 500 * CAP - 100_000);
        claim.inputs = vec![json!({"since":"0x0","previous_output":payout})];
        claim.cell_deps = exit.cell_deps.clone();
        claim.witnesses = vec![hx(packed::WitnessArgs::new_builder()
            .lock(Some(gw_types::bytes::Bytes::from(omni_signature(&[0; 65]))).pack())
            .build()
            .as_slice())];
        let mut wrong_key = claim.clone();
        sign_group(&mut wrong_key, &[0], 4, true);
        assert!(s.node.vm(&wrong_key).is_err());
        assert!(s.node.vm(&claim).is_err());
        sign_group(&mut claim, &[0], 1, true);
        let mut redirected = claim.clone();
        redirected.outputs[0].lock = Script::from_packed(profile.governance.clone()).unwrap();
        assert!(s.node.vm(&redirected).is_err());
        let claim_cycles = s.node.vm(&claim).unwrap();
        let received = point(&claim, 0);
        s.node.insert(claim.clone());
        assert!(!s.node.live.contains_key(&payout));
        assert!(
            s.node.vm(&claim).is_err(),
            "The same OmniLock payout cannot be claimed twice"
        );
        let received_cell = &s.node.live[&received];
        assert_eq!(received_cell.output.lock, wallet);
        assert_eq!(
            received_cell.output.type_script,
            payout_cell.output.type_script
        );
        assert_eq!(received_cell.output_data, payout_cell.output_data);
        assert_eq!(
            quantity(&received_cell.output.capacity).unwrap(),
            500 * CAP - 100_000
        );
        assert_eq!(
            read_state(&s),
            after,
            "An L1 user claim must not alter L2 state"
        );

        // Demonstrate that the user's CKB wallet can subsequently spend the assets.
        let mut spend = base_tx(
            vec![received_cell.output.clone()],
            vec![received_cell.output_data.clone()],
        );
        spend.outputs[0].capacity = format!("0x{:x}", 500 * CAP - 200_000);
        spend.inputs = vec![json!({"since":"0x0","previous_output":received})];
        spend.cell_deps = exit.cell_deps.clone();
        spend.witnesses = vec![hx(packed::WitnessArgs::new_builder()
            .lock(
                Some(gw_types::bytes::Bytes::from(
                    [multisig(5), vec![0; 65]].concat(),
                ))
                .pack(),
            )
            .build()
            .as_slice())];
        sign_group(&mut spend, &[0], 5, false);
        let wallet_cycles = s.node.vm(&spend).unwrap();
        s.node.insert(spend);
        assert!(!s.node.live.contains_key(&received));
        println!("Complete lifecycle token={with_token}: exit={exit_cycles}, OmniLock claim={claim_cycles}, CKB wallet spend={wallet_cycles} cycles");
    }
}

#[path = "support/settlement_e2e.rs"]
mod settlement_e2e;

#[path = "support/claim_e2e.rs"]
mod claim_e2e;

#[path = "support/refund_shutdown.rs"]
mod refund_shutdown;

#[path = "support/upgrade_e2e.rs"]
mod upgrade_e2e;
