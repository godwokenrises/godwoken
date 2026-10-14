mod support;
use godwoken_withdraw::{audit, db::Database, model::*, network::Rpc, report, schema::*};
use gw_common::state::{build_account_key, State, SUDT_TOTAL_SUPPLY_KEY};
use gw_types::{packed, prelude::*};
use rocksdb::{Options, DB};
use serde_json::json;
use std::{collections::BTreeMap, fs, path::Path, process::Command};
use support::{fixture, ETH};

fn run(path: &Path, b: &Bundle, eth: Option<[u8; 20]>, sum: bool) -> serde_json::Value {
    audit::run(&Database::open(path).unwrap(), b, eth, sum).unwrap()
}
#[test]
fn roots_backing_address_and_native_proofs() {
    let t = tempfile::tempdir().unwrap();
    let p = t.path().join("db");
    let b = fixture(&p, 0, 500);
    let r = run(&p, &b, Some(ETH), true);
    assert_eq!(r["overall_status"], "PASS");
    assert_eq!(r["assets"].as_array().unwrap().len(), 2);
    assert_eq!(r["registered_addresses_summed"], 1);
    assert_eq!(r["address_info"]["account_id"], 4);
    assert_eq!(
        r["address_assets"][0]["balance_l2_raw"],
        "1400000000000000000000"
    );
    assert_eq!(r["assets"][0]["custodian_total_raw"], "140000000000");
    assert!(r["authenticated_db_reads"].as_u64().unwrap() > 10);
}
#[test]
fn read_only_does_not_change_db() {
    fn fingerprint(p: &Path) -> BTreeMap<String, Vec<u8>> {
        fs::read_dir(p)
            .unwrap()
            .map(|e| {
                let p = e.unwrap().path();
                (
                    p.file_name().unwrap().to_string_lossy().into_owned(),
                    fs::read(p).unwrap(),
                )
            })
            .collect()
    }
    let t = tempfile::tempdir().unwrap();
    let p = t.path().join("db");
    let b = fixture(&p, 0, 500);
    let before = fingerprint(&p);
    run(&p, &b, Some(ETH), true);
    assert_eq!(before, fingerprint(&p));
}
#[test]
fn deficit_and_missing_pool_asset() {
    let t = tempfile::tempdir().unwrap();
    let p = t.path().join("db");
    let mut b = fixture(&p, 0, 500);
    b.cells.get_mut("custodian").unwrap()[1].output_data = hx(499u128.to_le_bytes());
    let r = run(&p, &b, None, false);
    assert_eq!(r["overall_status"], "FAIL");
    assert_eq!(r["assets"][1]["delta_l2_raw"], "-1");
    b.cells.get_mut("custodian").unwrap().pop();
    let r = run(&p, &b, None, false);
    assert_eq!(r["assets"].as_array().unwrap().len(), 2);
    assert_eq!(r["assets"][1]["status"], "DEFICIT");
}
#[test]
fn surplus_is_review() {
    let t = tempfile::tempdir().unwrap();
    let p = t.path().join("db");
    let mut b = fixture(&p, 0, 500);
    b.cells.get_mut("custodian").unwrap()[1].output_data = hx(501u128.to_le_bytes());
    assert_eq!(run(&p, &b, None, false)["overall_status"], "REVIEW");
}
#[test]
fn root_mismatch_is_not_comparable() {
    let t = tempfile::tempdir().unwrap();
    let p = t.path().join("db");
    let mut b = fixture(&p, 0, 500);
    let mut data = bytes(&b.rollup_cell.output_data).unwrap();
    data[32] ^= 1;
    b.rollup_cell.output_data = hx(data);
    let r = run(&p, &b, None, false);
    assert_eq!(r["overall_status"], "FAIL");
    assert!(r["assets"]
        .as_array()
        .unwrap()
        .iter()
        .all(|a| a["status"] == "NOT_COMPARABLE"));
}
#[test]
fn unknown_address_absence_proved() {
    let t = tempfile::tempdir().unwrap();
    let p = t.path().join("db");
    let b = fixture(&p, 0, 500);
    let r = run(&p, &b, Some([0xab; 20]), false);
    assert_eq!(r["address_info"]["status"], "unregistered");
    assert!(r["address_assets"]
        .as_array()
        .unwrap()
        .iter()
        .all(|a| a["balance_l2_raw"] == "0"));
}
#[test]
fn unmapped_pool_token_not_omitted() {
    let t = tempfile::tempdir().unwrap();
    let p = t.path().join("db");
    let mut b = fixture(&p, 0, 500);
    let mut c = b.cells["custodian"][1].clone();
    c.out_point.index = "0x2".into();
    c.output.type_script.as_mut().unwrap().args = hx([11; 32]);
    b.cells.get_mut("custodian").unwrap().push(c);
    let r = run(&p, &b, None, false);
    assert_eq!(r["overall_status"], "FAIL");
    assert!(r["assets"]
        .as_array()
        .unwrap()
        .iter()
        .any(|a| a["status"] == "UNMAPPED_CUSTODIAN"));
}
#[test]
fn duplicate_wrong_network_and_bad_token_rejected() {
    let t = tempfile::tempdir().unwrap();
    let p = t.path().join("db");
    let b = fixture(&p, 0, 500);
    let mut dup = b.clone();
    dup.cells
        .get_mut("custodian")
        .unwrap()
        .push(b.cells["custodian"][0].clone());
    assert!(dup.validate().is_err());
    let mut wrong = b.clone();
    wrong.network = "mainnet_v0".into();
    assert!(wrong.validate().is_err());
    let mut bad = b;
    bad.cells.get_mut("custodian").unwrap()[1].output_data = "0x01".into();
    assert!(audit::run(&Database::open(&p).unwrap(), &bad, None, false).is_err());
}
#[test]
fn deposit_not_counted_as_backing() {
    let t = tempfile::tempdir().unwrap();
    let p = t.path().join("db");
    let mut b = fixture(&p, 0, 500);
    let (_, cfg) = b.validate().unwrap();
    let mut c = b.cells["custodian"][0].clone();
    c.out_point.index = "0x9".into();
    c.output.lock.code_hash = cfg.hashes["deposit"].clone();
    b.cells.get_mut("deposit").unwrap().push(c);
    let r = run(&p, &b, None, false);
    assert_eq!(r["assets"][0]["delta_l2_raw"], "0");
    assert_eq!(
        r["assets"][0]["buckets"]["deposit"]["amount"],
        "100000000000"
    );
}
#[test]
fn ckb_fraction_not_rounded_away() {
    let t = tempfile::tempdir().unwrap();
    let p = t.path().join("db");
    let b = fixture(&p, 1, 500);
    let r = run(&p, &b, Some(ETH), false);
    assert_eq!(r["overall_status"], "FAIL");
    assert_eq!(r["assets"][0]["delta_l2_raw"], "-1");
    assert_eq!(r["address_assets"][0]["remainder_l2_raw"], "1");
}
#[test]
fn supply_sum_mismatch_fails() {
    let t = tempfile::tempdir().unwrap();
    let p = t.path().join("db");
    let b = fixture(&p, 0, 499);
    let r = run(&p, &b, None, true);
    assert_eq!(r["overall_status"], "FAIL");
    assert!(r["checks"]
        .as_array()
        .unwrap()
        .iter()
        .any(|c| c["name"] == "balance_sum_account_3" && c["status"] == "FAIL"));
}
#[test]
fn unfinalized_and_timestamp_finality() {
    let t = tempfile::tempdir().unwrap();
    let p = t.path().join("db");
    let mut b = fixture(&p, 0, 500);
    let (mut state, cfg) = b.validate().unwrap();
    assert!(audit::finalized(3200, &state, &cfg));
    assert!(!audit::finalized(3201, &state, &cfg));
    assert!(!audit::finalized((1 << 63) + 100, &state, &cfg));
    state.finalized_timepoint = (1 << 63) + 100;
    assert!(audit::finalized((1 << 63) + 100, &state, &cfg));
    assert!(!audit::finalized((1 << 63) + 101, &state, &cfg));
    let c = &mut b.cells.get_mut("custodian").unwrap()[1];
    let args = bytes(&c.output.lock.args).unwrap();
    let lock = packed::CustodianLockArgs::from_slice(&args[32..])
        .unwrap()
        .as_builder()
        .deposit_finalized_timepoint(20000u64.pack())
        .build();
    c.output.lock.args = hx([&args[..32], lock.as_slice()].concat());
    let r = run(&p, &b, None, false);
    assert_eq!(r["overall_status"], "REVIEW");
    assert_eq!(r["assets"][1]["status"], "AWAITING_FINALITY");
}
fn mutate(path: &Path, col: usize, key: &[u8], v: Option<&[u8]>) {
    let opts = Options::default();
    let names = DB::list_cf(&opts, path).unwrap();
    let db = DB::open_cf(&opts, path, names).unwrap();
    let cf = db.cf_handle(&col.to_string()).unwrap();
    if let Some(v) = v {
        db.put_cf(cf, key, v).unwrap()
    } else {
        db.delete_cf(cf, key).unwrap()
    }
    db.flush().unwrap();
}
#[test]
fn corrupted_leaf_fails_native_proof() {
    let t = tempfile::tempdir().unwrap();
    let p = t.path().join("db");
    fixture(&p, 0, 500);
    let key = build_account_key(3, &SUDT_TOTAL_SUPPLY_KEY);
    mutate(&p, COLUMN_ACCOUNT_SMT_LEAF, &key, Some(&[4; 32]));
    let db = Database::open(&p).unwrap();
    assert!(db.get_sudt_total_supply(3).is_err());
    assert!(db.raw_many(&[[0; 32], key]).is_err());
}
#[test]
fn malformed_root_branch_returns_error() {
    let t = tempfile::tempdir().unwrap();
    let p = t.path().join("db");
    fixture(&p, 0, 500);
    let mut key = vec![0; 32];
    key.push(255);
    mutate(&p, COLUMN_ACCOUNT_SMT_BRANCH, &key, Some(&[99]));
    assert!(Database::open(&p).is_err());
}
#[test]
fn wrong_db_identity_rejected() {
    let t = tempfile::tempdir().unwrap();
    let p = t.path().join("db");
    fixture(&p, 0, 500);
    mutate(&p, COLUMN_META, META_CHAIN_ID_KEY, Some(&[1; 32]));
    assert!(Database::open(&p).is_err());
}
#[test]
fn missing_db_is_never_created() {
    let t = tempfile::tempdir().unwrap();
    let p = t.path().join("missing");
    assert!(Database::open(&p).is_err());
    assert!(!p.exists());
}
#[test]
fn write_rpc_denied() {
    assert!(Rpc::new("http://localhost:1")
        .unwrap()
        .call("send_transaction", json!([]))
        .is_err());
}
#[test]
fn cli_db_query_is_offline_and_refuses_overwrite() {
    let t = tempfile::tempdir().unwrap();
    let db = t.path().join("db");
    fixture(&db, 0, 500);
    let out = t.path().join("l2.json");
    let launch = || {
        Command::new(env!("CARGO_BIN_EXE_gw-withdraw"))
            .args([
                "l2-db",
                "--protocol",
                "v1",
                "--db",
                db.to_str().unwrap(),
                "--eth-address",
                &hx(ETH),
                "--output",
                out.to_str().unwrap(),
            ])
            .env("http_proxy", "http://127.0.0.1:1")
            .env("https_proxy", "http://127.0.0.1:1")
            .output()
            .unwrap()
    };
    let r = launch();
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let r: serde_json::Value = serde_json::from_slice(&fs::read(&out).unwrap()).unwrap();
    assert_eq!(r["schema"], "l2-db-assets-1");
    assert_eq!(r["assets"].as_array().unwrap().len(), 2);
    assert!(r.get("observation").is_none());
    assert_eq!(launch().status.code(), Some(3));
}
#[test]
fn existing_output_and_db_nested_output_unchanged() {
    let t = tempfile::tempdir().unwrap();
    let p = t.path().join("db");
    fixture(&p, 0, 500);
    let existing = t.path().join("existing");
    fs::create_dir(&existing).unwrap();
    for out in [&existing, &p.join("output/nested")] {
        let r = Command::new(env!("CARGO_BIN_EXE_gw-withdraw"))
            .args([
                "l2-db",
                "--protocol",
                "v1",
                "--db",
                p.to_str().unwrap(),
                "--output",
                out.to_str().unwrap(),
            ])
            .output()
            .unwrap();
        assert_eq!(r.status.code(), Some(3));
    }
    assert_eq!(fs::read_dir(existing).unwrap().count(), 0);
    assert!(!p.join("output").exists());
}
#[test]
fn separated_reports_reconcile_and_reject_wrong_version_or_units() {
    use godwoken_withdraw::{asset_query as q, settlement::adapter::Protocol};
    let t = tempfile::tempdir().unwrap();
    let p = t.path().join("db");
    let b = fixture(&p, 0, 500);
    let l2 = q::l2_db(Protocol::V1, &p, Some(ETH)).unwrap();
    let l1 = q::l1_custodian(Protocol::V1, serde_json::to_value(b).unwrap()).unwrap();
    let r = q::check_backing(&l2, &l1).unwrap();
    assert!(r["assets"]
        .as_array()
        .unwrap()
        .iter()
        .all(|r| r["covers_selected_basis"] == true));
    let mut bad = l2.clone();
    bad["protocol"] = json!("v0");
    assert!(q::check_backing(&bad, &l1).is_err());
    let mut bad = l2.clone();
    bad["assets"][0]["l2_raw_per_l1_raw"] = json!("1");
    assert!(q::check_backing(&bad, &l1).is_err());
    let mut bad = l1.clone();
    bad["account_root"] = json!(hx([9; 32]));
    assert!(q::check_backing(&l2, &bad).is_err());
    let mut bad = l1.clone();
    let row = bad["assets"][0].clone();
    bad["assets"].as_array_mut().unwrap().push(row);
    assert!(q::check_backing(&l2, &bad).is_err());
}
#[test]
fn original_evidence_verified_in_rust() {
    report::verify_evidence(Path::new("tests/fixtures/mainnet-evidence")).unwrap();
}

#[test]
fn batch_authentication_preserves_order_and_proves_absence() {
    let t = tempfile::tempdir().unwrap();
    let p = t.path().join("db");
    fixture(&p, 0, 500);
    let db = Database::open(&p).unwrap();
    let keys = [
        [0; 32],
        build_account_key(3, &SUDT_TOTAL_SUPPLY_KEY),
        build_account_key(1, &SUDT_TOTAL_SUPPLY_KEY),
    ];
    let single = keys
        .iter()
        .map(|k| db.get_raw(k).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(db.raw_many(&keys).unwrap(), single);
    assert_eq!(single[0], [0; 32]);
    assert!(db.raw_many(&[keys[0], keys[0]]).is_err());
    assert!(db.raw_many(&[]).unwrap().is_empty());
}

#[test]
fn protocol_recipes_refuse_an_unapproved_snapshot_without_output() {
    let t = tempfile::tempdir().unwrap();
    let p = t.path().join("db");
    fixture(&p, 0, 500);
    let output = t.path().join("incomplete.json");
    let r = Command::new(env!("CARGO_BIN_EXE_gw-withdraw"))
        .args([
            "l2-db",
            "--protocol",
            "v1",
            "--db",
            p.to_str().unwrap(),
            "--protocol-state",
            "--output",
            output.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(r.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&r.stderr).contains("Unsupported refund snapshot root"));
    assert!(!output.exists());
}
