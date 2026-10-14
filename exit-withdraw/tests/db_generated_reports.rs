//! Compare already-generated DB-only output with a historical reference. This
//! test cannot influence generation: it receives only its finished report.
use anyhow::{ensure, Context, Result};
use num_bigint::BigUint as B;
use serde_json::{json, Value};
use std::{collections::BTreeMap, fs, path::Path};
fn csv(path: &Path) -> Result<Vec<BTreeMap<String, String>>> {
    csv::Reader::from_path(path)?
        .deserialize()
        .collect::<std::result::Result<_, _>>()
        .map_err(Into::into)
}
fn amount(s: &str, decimals: u64) -> Result<B> {
    let (a, b) = s.split_once('.').unwrap_or((s, ""));
    ensure!(b.len() <= decimals as usize, "Reference precision");
    Ok(format!("{a}{b}{}", "0".repeat(decimals as usize - b.len())).parse()?)
}
fn val(v: &Value) -> String {
    if let Some(s) = v.as_str() {
        s.into()
    } else {
        v.to_string()
    }
}
#[test]
#[ignore = "requires a completed DB-only report and comparison-only reference directory"]
fn v1_finished_report_matches_reference() -> Result<()> {
    let path = std::env::var("GW_GENERATED_REPORT")?;
    let actual: Value = serde_json::from_slice(&fs::read(path)?)?;
    ensure!(
        actual["protocol"] == "v1" && actual["complete_refund_calculation"] == true,
        "Incomplete V1 report"
    );
    let baseline = std::path::PathBuf::from(std::env::var("GW_REFERENCE_DIRECTORY")?);
    let mut checks = vec![];
    for (name, key, keys, fields) in [
        (
            "accounts.csv",
            "accounts",
            vec!["account_id", "asset_id"],
            vec![
                "entitlement_raw",
                "payable_l1_raw",
                "payable_l2_raw",
                "capacity_ready_l1_raw",
                "capacity_ready_l2_raw",
            ],
        ),
        (
            "capacity.csv",
            "capacity",
            vec!["account_id"],
            vec![
                "own_shannons",
                "minimum_shannons_assumption",
                "topup_shannons",
            ],
        ),
        (
            "source-reserves.csv",
            "source_reserves",
            vec!["source_account_id", "asset_id"],
            vec!["native_input_raw", "refund_entitlement_raw", "retained_raw"],
        ),
    ] {
        let mut expected = BTreeMap::new();
        for row in csv(&baseline.join(name))? {
            let k = keys.iter().map(|k| row[*k].clone()).collect::<Vec<_>>();
            ensure!(
                expected
                    .insert(
                        k,
                        fields
                            .iter()
                            .map(|k| row[*k].parse::<B>())
                            .collect::<std::result::Result<Vec<_>, _>>()?
                    )
                    .is_none(),
                "Duplicate reference row"
            );
        }
        let mut generated = BTreeMap::new();
        for row in actual[key].as_array().context("Report rows")? {
            let k = keys.iter().map(|k| val(&row[*k])).collect::<Vec<_>>();
            ensure!(
                generated
                    .insert(
                        k,
                        fields
                            .iter()
                            .map(|k| val(&row[*k]).parse::<B>())
                            .collect::<std::result::Result<Vec<_>, _>>()?
                    )
                    .is_none(),
                "Duplicate generated row"
            );
        }
        let mut differences = vec![];
        for k in expected
            .keys()
            .chain(generated.keys())
            .collect::<std::collections::BTreeSet<_>>()
        {
            if expected.get(k) != generated.get(k) {
                differences.push(json!({"key":k,"expected":expected.get(k).map(|a|a.iter().map(ToString::to_string).collect::<Vec<_>>()),"actual":generated.get(k).map(|a|a.iter().map(ToString::to_string).collect::<Vec<_>>())}));
            }
        }
        checks.push(json!({"table":name,"generated_rows":generated.len(),"reference_rows":expected.len(),"differences":differences}));
    }
    let mut expected = BTreeMap::<(String, String, String), B>::new();
    for r in csv(&baseline.join("claim-sources.csv"))? {
        *expected
            .entry((
                r["source_account_id"].clone(),
                r["account_id"].clone(),
                r["asset_id"].clone(),
            ))
            .or_default() += r["entitlement_raw"].parse::<B>()?;
    }
    let mut generated = BTreeMap::<(String, String, String), B>::new();
    for r in actual["claim_sources"].as_array().context("Claims")? {
        *generated
            .entry((
                val(&r["source_account_id"]),
                val(&r["account_id"]),
                val(&r["asset_id"]),
            ))
            .or_default() += val(&r["entitlement_raw"]).parse::<B>()?;
    }
    let mut diff = vec![];
    for k in expected
        .keys()
        .chain(generated.keys())
        .collect::<std::collections::BTreeSet<_>>()
    {
        if expected.get(k) != generated.get(k) {
            diff.push(json!({"key":k,"expected":expected.get(k).map(ToString::to_string),"actual":generated.get(k).map(ToString::to_string)}));
        }
    }
    checks.push(json!({"table":"claim-sources.csv","comparison":"Exact source/recipient/asset sums; protocol path explanations are regenerated","reference_groups":expected.len(),"generated_groups":generated.len(),"differences":diff}));
    let mut diffs = vec![];
    let reference_assets = csv(&baseline.join("assets.csv"))?;
    for a in &reference_assets {
        let asset: u64 = a["asset_id"].parse()?;
        let g = actual["assets"]
            .as_array()
            .unwrap()
            .iter()
            .find(|g| g["asset_id"] == asset)
            .context("Asset")?;
        let d = g["decimals"].as_u64().context("Decimals")?;
        for (old, new) in [
            ("combined_entitlement", "refund_entitlement_raw"),
            (
                "combined_payable_after_l1_rounding",
                "refund_payable_l2_raw",
            ),
            ("capacity_ready_payout", "capacity_ready_l2_raw"),
            ("l2_recorded_before", "l2_recorded_before_raw"),
            (
                "l2_recorded_after_all_payable",
                "l2_recorded_after_payable_raw",
            ),
            (
                "discovered_balance_after_all_payable",
                "discovered_l2_after_payable_raw",
            ),
        ] {
            let x = amount(&a[old], d)?;
            let y = val(&g[new]).parse::<B>()?;
            if x != y {
                diffs.push(json!({"asset_id":asset,"field":old,"expected":x.to_string(),"actual":y.to_string()}));
            }
        }
    }
    for a in actual["assets"].as_array().context("Assets")? {
        let asset = a["asset_id"].as_u64().context("Asset id")?;
        if !reference_assets
            .iter()
            .any(|r| r["asset_id"] == asset.to_string())
        {
            for field in [
                "l2_recorded_before_raw",
                "discovered_l2_before_raw",
                "refund_entitlement_raw",
                "refund_payable_l2_raw",
                "discovered_l2_retained_raw",
            ] {
                if val(&a[field]).parse::<B>()? != B::default() {
                    diffs.push(
                        json!({"asset_id":asset,"field":field,"unexpected_nonzero_asset":true}),
                    );
                }
            }
        }
    }
    checks.push(json!({"table":"assets.csv","generated_native_definitions":actual["assets"].as_array().unwrap().len(),"reference_active_assets":reference_assets.len(),"additional_definitions_required_zero":true,"differences":diffs}));
    let pass = checks
        .iter()
        .all(|c| c["differences"].as_array().is_some_and(Vec::is_empty));
    if let Ok(out) = std::env::var("GW_COMPARISON_OUTPUT") {
        godwoken_withdraw::report::write_new(
            Path::new(&out),
            &serde_json::to_vec_pretty(
                &json!({"protocol":"v1","root":actual["root"],"pass":pass,"checks":checks,"historical_tables_used_only_after_generation":true,"custodian_fields_not_generated_offline":true}),
            )?,
        )?;
    }
    ensure!(
        pass,
        "DB-only report differs from reference: {}",
        serde_json::to_string(&checks)?
    );
    Ok(())
}
