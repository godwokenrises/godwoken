#[allow(dead_code)]
mod support;
use godwoken_withdraw::{contract_inventory, db::Database, model::*};
use gw_types::prelude::*;
use serde_json::Value;
use std::fs;

#[test]
fn inventory_excludes_eoa_and_keeps_unregistered_accounts_unresolved() {
    let t = tempfile::tempdir().unwrap();
    let path = t.path().join("db");
    let bundle = support::fixture(&path, 0, 500);
    let db = Database::open(&path).unwrap();
    let root = db.root();
    let out = t.path().join("inventory");
    let summary = contract_inventory::run(&db, &bundle, &out).unwrap();
    assert_eq!(summary["counts"]["eth_eoa"], 1);
    assert_eq!(summary["non_eoa_accounts"], 4);
    assert_eq!(summary["backed_asset_count"], 2);
    assert_eq!(summary["balance_rows"], 0);
    let accounts: Vec<Value> =
        serde_json::from_slice(&fs::read(out.join("accounts.json")).unwrap()).unwrap();
    assert!(accounts.iter().all(|a| a["account_id"] != 4));
    assert!(accounts
        .iter()
        .all(|a| a["balance_status"] == "unresolved_no_registry"
            && a["positive_backed_asset_count"].is_null()));
    assert!(out.join("SHA256SUMS").exists());
    assert_eq!(db.root(), root);
    assert!(contract_inventory::run(&db, &bundle, &out).is_err());
}
#[test]
fn script_classification_uses_config_and_hash_type() {
    let t = tempfile::tempdir().unwrap();
    let bundle = support::fixture(&t.path().join("db"), 0, 500);
    let (_, cfg) = bundle.validate().unwrap();
    for allowed in cfg.packed.allowed_contract_type_hashes().into_iter() {
        let s = Script {
            code_hash: hx(allowed.hash().as_slice()),
            hash_type: "type".into(),
            args: ROLLUP_HASH.into(),
        };
        let kind = contract_inventory::account_kind(&s, &cfg).unwrap();
        let expected = match u8::from(allowed.type_()) {
            1 => "system_meta",
            2 => "system_sudt",
            3 => "polyjuice",
            4 => "system_registry",
            _ => "unknown",
        };
        assert_eq!(kind, expected);
        let wrong = Script {
            hash_type: "data".into(),
            ..s
        };
        assert_eq!(
            contract_inventory::account_kind(&wrong, &cfg).unwrap(),
            "unknown"
        );
    }
}
#[test]
fn mismatched_snapshot_cannot_emit_inventory() {
    let t = tempfile::tempdir().unwrap();
    let path = t.path().join("db");
    let mut bundle = support::fixture(&path, 0, 500);
    bundle.rollup_cell.output_data = "0x00".into();
    let out = t.path().join("invalid");
    assert!(contract_inventory::run(&Database::open(&path).unwrap(), &bundle, &out).is_err());
    assert!(!out.exists());
}

#[test]
fn missing_code_keeps_authenticated_contract_balances() {
    let t = tempfile::tempdir().unwrap();
    let path = t.path().join("db");
    let bundle = support::fixture_missing_contract_code(&path);
    let out = t.path().join("inventory");
    let result = contract_inventory::run(&Database::open(&path).unwrap(), &bundle, &out).unwrap();
    assert_eq!(result["code_evidence_complete"], false);
    assert_eq!(
        result["accounts_with_missing_runtime_code"],
        serde_json::json!([4])
    );
    assert_eq!(result["counts"]["polyjuice_runtime"], 1);
    assert_eq!(result["balance_rows"], 2);
    let rows: Vec<Value> =
        serde_json::from_slice(&fs::read(out.join("balances.json")).unwrap()).unwrap();
    assert!(rows.iter().all(|r| r["account_id"] == 4));
    assert!(rows
        .iter()
        .any(|r| r["asset_id"] == 3 && r["balance_l2_raw"] == "500"));
    let accounts: Vec<Value> =
        serde_json::from_slice(&fs::read(out.join("accounts.json")).unwrap()).unwrap();
    let contract = accounts.iter().find(|a| a["account_id"] == 4).unwrap();
    assert_eq!(contract["positive_backed_token_count"], 1);
    assert_eq!(contract["code_preimage_status"], "missing_preimage");
    assert_eq!(contract["balance_status"], "authenticated");
}

#[test]
fn empty_code_hash_is_not_missing_bytecode() {
    let t = tempfile::tempdir().unwrap();
    let path = t.path().join("db");
    let b = support::fixture_empty_contract_code(&path);
    let out = t.path().join("inventory");
    let result = contract_inventory::run(&Database::open(&path).unwrap(), &b, &out).unwrap();
    assert_eq!(result["code_evidence_complete"], true);
    assert_eq!(result["counts"]["polyjuice_empty_code"], 1);
    assert_eq!(
        result["accounts_with_missing_runtime_code"],
        serde_json::json!([])
    );
    assert!(fs::read(
        out.join("runtime-code")
            .join(format!("{}.bin", hx(hash(&[]))))
    )
    .unwrap()
    .is_empty());
}
