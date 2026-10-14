//! Optional real-DB regression. The baseline is read only AFTER allocation is calculated.
//! GW_DB_FACTS must be a newly generated `l2-db --protocol-state` report.
use anyhow::{Context, Result};
use godwoken_withdraw::refund;
use num_bigint::BigUint;
use serde_json::{json, Value};
use std::{collections::BTreeMap, fs, path::PathBuf};

#[test]
#[ignore = "requires fresh authenticated DB facts and a comparison-only baseline"]
fn v0_allocations_from_fresh_db_facts() -> Result<()> {
    let facts = PathBuf::from(std::env::var("GW_DB_FACTS")?);
    let v: Value = serde_json::from_slice(&fs::read(facts)?)?;
    let actual = refund::v0::reconstruct(&v, &v["protocol_state"])?;
    let tables = refund::reports::v0(&actual)?;
    let mut result = BTreeMap::<(u64, u64), BigUint>::new();
    for a in actual["accounts"].as_array().context("accounts")? {
        let k = (
            a["account_id"].as_u64().context("account")?,
            a["asset_id"].as_u64().context("asset")?,
        );
        assert!(result
            .insert(k, a["entitlement_raw"].as_str().context("amount")?.parse()?)
            .is_none());
    }
    // Nothing read below can influence the reconstructed calculation above.
    let baseline = PathBuf::from(std::env::var("GW_REFERENCE_ACCOUNTS")?);
    let mut expected = BTreeMap::<(u64, u64), BigUint>::new();
    for r in csv::Reader::from_path(baseline)?.deserialize::<BTreeMap<String, String>>() {
        let r = r?;
        let amount: BigUint = r["entitlement_raw"].parse()?;
        if amount != BigUint::default() {
            assert!(expected
                .insert((r["account_id"].parse()?, r["asset_id"].parse()?), amount)
                .is_none());
        }
    }
    let mut differences = vec![];
    for k in result
        .keys()
        .chain(expected.keys())
        .collect::<std::collections::BTreeSet<_>>()
    {
        let a = result.get(k).cloned().unwrap_or_default();
        let b = expected.get(k).cloned().unwrap_or_default();
        if a != b {
            differences.push(json!({"account_id":k.0,"asset_id":k.1,"generated_raw":a.to_string(),"reference_raw":b.to_string()}));
        }
    }
    let capacity_path = PathBuf::from(std::env::var("GW_REFERENCE_CAPACITY")?);
    let mut expected_capacity = BTreeMap::new();
    for row in csv::Reader::from_path(capacity_path)?.deserialize::<BTreeMap<String, String>>() {
        let row = row?;
        let owner: u64 = row["account_id"].parse()?;
        let amounts = [
            "own_shannons",
            "minimum_shannons_assumption",
            "topup_shannons",
        ]
        .map(|k| row[k].parse::<BigUint>().unwrap());
        assert!(expected_capacity.insert(owner, amounts).is_none());
    }
    let mut actual_capacity = BTreeMap::new();
    for row in actual["capacity"].as_array().context("capacity")? {
        let owner = row["account_id"].as_u64().context("account")?;
        let amounts = [
            "own_shannons",
            "minimum_shannons_assumption",
            "topup_shannons",
        ]
        .map(|k| row[k].as_str().unwrap().parse::<BigUint>().unwrap());
        assert!(actual_capacity.insert(owner, amounts).is_none());
    }
    let capacity_equal = actual_capacity == expected_capacity;
    let sources_path = PathBuf::from(std::env::var("GW_REFERENCE_SOURCES")?);
    let mut expected_sources = BTreeMap::new();
    for row in csv::Reader::from_path(sources_path)?.deserialize::<BTreeMap<String, String>>() {
        let row = row?;
        let key = (row["source_account_id"].clone(), row["asset_id"].clone());
        let amounts = ["native_input_raw", "refund_entitlement_raw", "retained_raw"]
            .map(|k| row[k].parse::<BigUint>().unwrap());
        assert!(expected_sources.insert(key, amounts).is_none());
    }
    let mut source_differences = vec![];
    let mut generated_sources = std::collections::BTreeSet::new();
    for row in actual["sources"].as_array().context("sources")? {
        let key = (
            row["source_account_id"]
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| row["source_account_id"].as_u64().unwrap().to_string()),
            row["asset_id"].as_u64().unwrap().to_string(),
        );
        let amounts = ["native_input_raw", "refund_entitlement_raw", "retained_raw"]
            .map(|k| row[k].as_str().unwrap().parse::<BigUint>().unwrap());
        if expected_sources.get(&key) != Some(&amounts) {
            source_differences.push(json!({"source":key,"actual":amounts.map(|n|n.to_string()),"expected":expected_sources.get(&key).map(|a|a.clone().map(|n|n.to_string()))}));
        }
        assert!(generated_sources.insert(key));
    }
    let missing_sources = expected_sources
        .keys()
        .filter(|k| !generated_sources.contains(*k))
        .cloned()
        .collect::<Vec<_>>();
    let reference_claims = PathBuf::from(std::env::var("GW_REFERENCE_CLAIMS")?);
    let mut table_differences = vec![];
    for (table, path, keys) in [
        (
            "accounts",
            PathBuf::from(std::env::var("GW_REFERENCE_ACCOUNTS")?),
            vec!["account_id", "asset_id"],
        ),
        (
            "capacity",
            PathBuf::from(std::env::var("GW_REFERENCE_CAPACITY")?),
            vec!["account_id"],
        ),
        (
            "source_reserves",
            PathBuf::from(std::env::var("GW_REFERENCE_SOURCES")?),
            vec!["source_account_id", "asset_id"],
        ),
        (
            "claim_sources",
            reference_claims,
            vec![
                "account_id",
                "asset_id",
                "source_account_id",
                "protocol_path",
            ],
        ),
    ] {
        let mut reader = csv::Reader::from_path(path)?;
        let headers = reader
            .headers()?
            .iter()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        let expected = reader
            .deserialize::<BTreeMap<String, String>>()
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let rows = tables[table].as_array().context("Report rows")?;
        let stringify = |v: &Value| match v {
            Value::Null => String::new(),
            Value::String(s) => s.clone(),
            v => v.to_string(),
        };
        let mut index = BTreeMap::new();
        for row in rows {
            let key = keys.iter().map(|k| stringify(&row[*k])).collect::<Vec<_>>();
            assert!(index.insert(key, row).is_none(), "Duplicate report key");
        }
        for row in expected {
            let key = keys.iter().map(|k| row[*k].clone()).collect::<Vec<_>>();
            if let Some(actual) = index.remove(&key) {
                for field in &headers {
                    let generated = stringify(&actual[field]);
                    if generated != row[field] {
                        table_differences.push(json!({"table":table,"key":key,"field":field,"expected":row[field],"generated":generated}));
                    }
                }
            } else {
                table_differences.push(json!({"table":table,"missing_key":key}));
            }
        }
        for key in index.keys() {
            table_differences.push(json!({"table":table,"unexpected_key":key}));
        }
    }
    let mut asset_differences = vec![];
    for row in csv::Reader::from_path(std::env::var("GW_REFERENCE_ASSETS")?)?
        .deserialize::<BTreeMap<String, String>>()
    {
        let row = row?;
        let aid: u64 = row["asset_id"].parse()?;
        let actual = tables["assets"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["asset_id"] == aid)
            .context("Missing asset summary")?;
        let decimals = actual["decimals"].as_u64().unwrap() as usize;
        for (reference, field) in [
            ("l2_recorded_before", "discovered_l2_before_raw"),
            ("combined_entitlement", "refund_entitlement_raw"),
            (
                "discovered_balance_after_all_payable",
                "discovered_l2_retained_raw",
            ),
        ] {
            let value = &row[reference];
            let (whole, fraction) = value.split_once('.').unwrap_or((value, ""));
            anyhow::ensure!(
                fraction.len() <= decimals,
                "Reference precision exceeds raw unit"
            );
            let value: BigUint =
                format!("{whole}{fraction}{}", "0".repeat(decimals - fraction.len())).parse()?;
            let generated: BigUint = actual[field].as_str().unwrap().parse()?;
            if value != generated {
                asset_differences.push(json!({"asset_id":aid,"field":field,"reference_raw":value.to_string(),"generated_raw":generated.to_string()}));
            }
        }
    }
    let mut authority_differences = vec![];
    let expected_authorities = csv::Reader::from_path(std::env::var("GW_REFERENCE_AUTHORITIES")?)?
        .deserialize::<BTreeMap<String, String>>()
        .collect::<std::result::Result<Vec<_>, _>>()?;
    for actual in tables["authorities"].as_array().unwrap() {
        let aid = actual["account_id"].as_u64().unwrap();
        let expected = expected_authorities
            .iter()
            .find(|r| r["version"] == "v0" && r["account_id"] == aid.to_string())
            .context("Missing baseline authority")?;
        let old = expected["authority_addresses_and_roles"]
            .split("; ")
            .map(|s| s.strip_prefix("signer/owner: ").context("Authority role"))
            .collect::<Result<std::collections::BTreeSet<_>>>()?;
        let new = actual["owners"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["eth_address"].as_str().unwrap())
            .collect::<std::collections::BTreeSet<_>>();
        if old != new
            || expected["refund_rule"]
                != format!(
                    "{}-of-{} distinct authenticated owners",
                    actual["refund_threshold"], actual["members"]
                )
        {
            authority_differences
                .push(json!({"account_id":aid,"reference":expected,"generated":actual}));
        }
    }
    let output = PathBuf::from(std::env::var("GW_REGRESSION_OUTPUT")?);
    assert!(!output.exists(), "Refuse to overwrite regression output");
    let report = json!({"scope":"All nonzero per-beneficiary/asset entitlement integers; capacity own/minimum/topup also compared; not yet complete report/provenance equivalence","all_equal":differences.is_empty() && capacity_equal && source_differences.is_empty() && missing_sources.is_empty() && table_differences.is_empty() && asset_differences.is_empty() && authority_differences.is_empty(),"asset_differences":asset_differences,"authority_differences":authority_differences,"table_differences":table_differences,"offline_tables_equal":table_differences.is_empty(),"tables":tables,"registered_source_differences":source_differences,"registered_source_rows":generated_sources.len(),"not_yet_reconstructed_source_keys":missing_sources,"full_report_equivalence":false,"capacity_equal":capacity_equal,"capacity_accounts":actual_capacity.len(),"generated_rows":result.len(),"reference_rows":expected.len(),"differences":differences,"reconstruction":actual});
    fs::write(output, serde_json::to_vec_pretty(&report)?)?;
    assert!(asset_differences.is_empty(), "Asset summaries differ");
    assert!(authority_differences.is_empty(), "Authority owners differ");
    assert!(missing_sources.is_empty(), "Missing native sources");
    assert!(
        table_differences.is_empty(),
        "Report table fields differ: {}",
        table_differences.len()
    );
    assert!(capacity_equal, "Capacity comparison differs");
    assert!(
        source_differences.is_empty(),
        "Registered native source accounting differs"
    );
    assert!(
        differences.is_empty(),
        "DB reconstruction differs from baseline: {} account/asset rows",
        differences.len()
    );
    Ok(())
}
