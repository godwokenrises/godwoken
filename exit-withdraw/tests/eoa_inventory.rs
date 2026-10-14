mod support;
use godwoken_withdraw::{db::Database, eoa_inventory};
#[test]
fn export_authenticates_balances_and_refuses_replacement() {
    let t = tempfile::tempdir().unwrap();
    let path = t.path().join("db");
    let b = support::fixture(&path, 0, 500);
    let db = Database::open(&path).unwrap();
    let root = db.root();
    let out = t.path().join("eoa");
    let r = eoa_inventory::run(&db, &b, &out).unwrap();
    assert_eq!(r["eoa_accounts"], 1);
    assert_eq!(r["balance_queries"], 2);
    assert_eq!(r["funded_eoa_accounts"], 1);
    let totals: serde_json::Value =
        serde_json::from_slice(&std::fs::read(out.join("asset-totals.json")).unwrap()).unwrap();
    assert_eq!(
        totals
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["asset_id"] == 3)
            .unwrap()["eoa_balance_raw"],
        "500"
    );
    assert_eq!(db.root(), root);
    assert!(eoa_inventory::run(&db, &b, &out).is_err());
}
#[test]
fn wrong_root_and_db_output_are_rejected() {
    let t = tempfile::tempdir().unwrap();
    let path = t.path().join("db");
    let mut b = support::fixture(&path, 0, 500);
    let db = Database::open(&path).unwrap();
    assert!(eoa_inventory::run(&db, &b, &path.join("report")).is_err());
    b.rollup_cell.output_data = "0x00".into();
    let out = t.path().join("bad");
    assert!(eoa_inventory::run(&db, &b, &out).is_err());
    assert!(!out.exists());
}
#[test]
fn exact_decimal_formatting() {
    assert_eq!(
        eoa_inventory::decimal("1", Some(18)).unwrap(),
        "0.000000000000000001"
    );
    assert_eq!(
        eoa_inventory::decimal("9007199254740993123456789", Some(6)).unwrap(),
        "9007199254740993123.456789"
    );
    assert!(eoa_inventory::decimal("100", None).is_none());
}
