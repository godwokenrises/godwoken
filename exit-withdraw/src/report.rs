use crate::model::*;
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{fs, io::Write, path::Path};
fn text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => "not available".into(),
        _ => v.to_string(),
    }
}
fn row(values: &[String]) -> String {
    format!(
        "| {} |\n",
        values
            .iter()
            .map(|s| s.replace('|', "\\|").replace('\n', " "))
            .collect::<Vec<_>>()
            .join(" | ")
    )
}
pub fn markdown(r: &Value) -> String {
    let mut s=format!("# Godwoken mainnet v1 state and backing audit\n\nOverall: **{}**\n\nGenerated: {}\n\nImplementation: Rust / native Godwoken state and SMT components.\n\nDB: `{}`\n\nDB SMT root: `{}`\n\nL1 account root: `{}`\n\n## State checks\n\n| Check | Result | Detail |\n| --- | --- | --- |\n",text(&r["overall_status"]),text(&r["generated_at"]),text(&r["db_path"]),text(&r["db_smt_root"]),text(&r["l1_state"]["account_root"]));
    for c in r["checks"].as_array().into_iter().flatten() {
        s += &row(&[text(&c["name"]), text(&c["status"]), text(&c["detail"])]);
    }
    s+="\n## All L2 sUDT assets and backing\n\nAll amounts are integer raw units. CKB L2 units have 18 decimals; custodian CKB units are shannons (8 decimals). Delta uses L2 units.\n\n| Asset / L1 type hash | L2 ID | L2 total supply | Registered balance sum | Finalized custodian | Unfinalized custodian | Delta (L2) | Result |\n| --- | --- | --- | --- | --- | --- | --- | --- |\n";
    for a in r["assets"].as_array().into_iter().flatten() {
        let label = format!("{} `{}`", text(&a["symbol"]), text(&a["asset_id"]));
        let mut vals = vec![label];
        for k in [
            "l2_account_id",
            "l2_total_supply_raw",
            "l2_registered_balance_sum_raw",
            "custodian_finalized_raw",
            "custodian_unfinalized_raw",
            "delta_l2_raw",
            "status",
        ] {
            vals.push(text(&a[k]));
        }
        s += &row(&vals);
    }
    s+="\n## Separate L1 categories\n\nDeposits, withdrawals, stake and challenge cells are not added to custodian backing. Occupied capacity is recorded in the JSON cell inventory.\n\n| Asset | Deposit | Withdrawal | Stake | Challenge |\n| --- | --- | --- | --- | --- |\n";
    for a in r["assets"].as_array().into_iter().flatten() {
        let mut vals = vec![format!("{} `{}`", text(&a["symbol"]), text(&a["asset_id"]))];
        for k in ["deposit", "withdrawal", "stake", "challenge"] {
            vals.push(a["buckets"][k]["amount"].as_str().unwrap_or("0").into());
        }
        s += &row(&vals);
    }
    if !r["eth_address"].is_null() {
        s+=&format!("\n## ETH address assets\n\nAddress: `{}`; registry: **{}**. Every L2 sUDT is listed, including zero balances.\n\n| Asset / type hash | L2 ID | L2 raw balance | L1 floor | Remainder (L2 raw) | Display |\n| --- | --- | --- | --- | --- | --- |\n",text(&r["eth_address"]),text(&r["address_info"]["status"]));
        for a in r["address_assets"].as_array().into_iter().flatten() {
            let mut vals = vec![format!("{} `{}`", text(&a["symbol"]), text(&a["asset_id"]))];
            for k in [
                "l2_account_id",
                "balance_l2_raw",
                "amount_l1_floor",
                "remainder_l2_raw",
                "display",
            ] {
                vals.push(text(&a[k]));
            }
            s += &row(&vals);
        }
    }
    for (title, key) in [("Findings", "findings"), ("Limits", "limitations")] {
        s += &format!("\n## {title}\n\n");
        let list = r[key].as_array();
        if list.is_some_and(|a| a.is_empty()) {
            s += "No additional discrepancies found within the checked scope.\n";
        }
        for f in list.into_iter().flatten() {
            s += &format!("- {}\n", text(f));
        }
    }
    s
}
pub fn write_new(path: &Path, data: &[u8]) -> Result<()> {
    let mut f = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .with_context(|| format!("Create {} (refusing overwrite)", path.display()))?;
    f.write_all(data)?;
    Ok(())
}
fn csv(rows: &Value, fields: &[&str]) -> Result<Vec<u8>> {
    let mut writer = csv::Writer::from_writer(Vec::new());
    writer.write_record(fields)?;
    for r in rows.as_array().context("Expected report rows")? {
        writer.write_record(fields.iter().map(|f| text(&r[*f])))?;
    }
    Ok(writer.into_inner()?)
}
pub fn write_reports(dir: &Path, r: &Value, bundle: &Bundle) -> Result<()> {
    write_new(&dir.join("report.json"), &serde_json::to_vec_pretty(r)?)?;
    write_new(&dir.join("report.md"), markdown(r).as_bytes())?;
    write_new(
        &dir.join("l1-snapshot.json"),
        &serde_json::to_vec_pretty(bundle)?,
    )?;
    let fields = [
        "asset_id",
        "symbol",
        "l2_account_id",
        "l2_total_supply_raw",
        "l2_registered_balance_sum_raw",
        "l2_raw_per_l1_raw",
        "l2_liability_l1_floor",
        "l2_remainder_raw",
        "custodian_finalized_raw",
        "custodian_unfinalized_raw",
        "custodian_total_raw",
        "delta_l2_raw",
        "finalized_delta_l2_raw",
        "status",
    ];
    write_new(&dir.join("assets.csv"), &csv(&r["assets"], &fields)?)?;
    if !r["eth_address"].is_null() {
        write_new(
            &dir.join("address-assets.csv"),
            &csv(
                &r["address_assets"],
                &[
                    "asset_id",
                    "symbol",
                    "l2_account_id",
                    "balance_l2_raw",
                    "amount_l1_floor",
                    "remainder_l2_raw",
                    "display",
                ],
            )?,
        )?;
    }
    let mut paths: Vec<_> = fs::read_dir(dir)?
        .map(|p| p.map(|p| p.path()))
        .collect::<std::io::Result<_>>()?;
    paths.sort();
    let mut sums = String::new();
    for p in paths {
        if p.is_file() {
            sums += &format!(
                "{}  {}\n",
                hex::encode(Sha256::digest(fs::read(&p)?)),
                p.file_name()
                    .context("No artifact filename")?
                    .to_string_lossy()
            );
        }
    }
    write_new(&dir.join("SHA256SUMS"), sums.as_bytes())?;
    Ok(())
}

/// Verify historical research evidence without Python, RPC or a production DB.
pub fn verify_evidence(dir: &Path) -> Result<()> {
    let read = |name: &str| -> Result<Value> {
        Ok(serde_json::from_slice(&fs::read(
            dir.join(format!("{name}.json")),
        )?)?)
    };
    for line in fs::read_to_string(dir.join("SHA256SUMS"))?.lines() {
        let (expected, name) = line.split_once("  ").context("Bad checksum manifest")?;
        ensure!(
            !name.contains('/') && !name.contains('\\'),
            "Unsafe manifest path"
        );
        ensure!(
            hex::encode(Sha256::digest(fs::read(dir.join(name))?)) == expected,
            "Checksum mismatch: {name}"
        );
    }
    for net in ["v0", "v1"] {
        let raw = read(&format!("{net}-rollup"))?;
        let cell = &raw["response"]["result"]["objects"][0];
        let state = GlobalState::parse(&bytes(
            cell["output_data"].as_str().context("Missing state")?,
        )?)?;
        let live = read(&format!("{net}-rollup-live"))?;
        ensure!(
            live["response"]["result"]["status"] == "live"
                && live["response"]["result"]["cell"]["data"]["content"] == cell["output_data"],
            "Saved rollup/liveness data mismatch"
        );
        let web = read(&format!("{net}-web3"))?;
        let tip = quantity(
            web["response"]["result"]
                .as_str()
                .context("Missing Web3 tip")?,
        )?;
        ensure!(
            state.block_count - 1 == tip + if net == "v0" { 1 } else { 0 },
            "Unexpected Web3/rollup height difference"
        );
        if net == "v1" {
            let config = read("v1-config-cell")?;
            let data = config["response"]["result"]["cell"]["data"]["content"]
                .as_str()
                .context("Missing config")?;
            ensure!(
                config["response"]["result"]["status"] == "live"
                    && hx(hash(&bytes(data)?)) == state.config_hash,
                "Config hash mismatch"
            );
            let b = read("v1-l2-tip-block-by-hash")?;
            let raw = &b["response"]["result"]["block"]["raw"];
            ensure!(
                raw["post_account"]["merkle_root"] == state.account_root
                    && quantity(
                        raw["post_account"]["count"]
                            .as_str()
                            .context("Missing account count")?
                    )? == state.account_count as u64
                    && quantity(raw["number"].as_str().context("Missing block number")?)? + 1
                        == state.block_count,
                "L2 post-state cross-check failed"
            );
        }
    }
    for name in ["state_validator", "custodian_lock", "omni_lock"] {
        let live = read(&format!("v1-code-live-{name}"))?;
        let live = &live["response"]["result"];
        ensure!(live["status"] == "live", "Saved code not live");
        let data = &live["cell"]["data"];
        ensure!(
            hx(hash(&bytes(
                data["content"].as_str().context("Missing code data")?
            )?)) == data["hash"].as_str().context("Missing code hash")?,
            "Code hash mismatch"
        );
        let current = read(&format!("v1-current-{name}"))?;
        ensure!(
            current["response"]["result"]["objects"][0]["output"] == live["cell"]["output"],
            "Code cell search/live mismatch"
        );
    }
    let summary: Value = serde_json::from_slice(&fs::read(dir.join("v1-inventory-summary.json"))?)?;
    for (family, expected) in summary.as_object().context("Invalid inventory summary")? {
        let mut groups: BTreeMap<String, Value> = BTreeMap::new();
        let mut count = 0u64;
        let mut seen = BTreeSet::new();
        for page in 0..10000 {
            let r = read(&format!("v1-{family}-page-{page}"))?;
            let cells = r["response"]["result"]["objects"]
                .as_array()
                .context("Missing inventory page")?;
            if cells.is_empty() {
                break;
            }
            for c in cells {
                ensure!(
                    seen.insert(c["out_point"].to_string()),
                    "Duplicate inventory outpoint"
                );
                let token = &c["output"]["type"];
                // Old Python summary used JSON Script strings with spaces; normalize below.
                let key = if token.is_null() {
                    "CKB-only".into()
                } else {
                    token.to_string()
                };
                let g=groups.entry(key).or_insert(json!({"cells":0,"capacity_shannons":"0","sudt_amount":"0","unexpected_data":0}));
                g["cells"] = json!(g["cells"].as_u64().unwrap() + 1);
                let cap: g_num::Big = g_num::parse(&g["capacity_shannons"])?;
                g["capacity_shannons"] = json!((cap
                    + quantity(
                        c["output"]["capacity"]
                            .as_str()
                            .context("Missing capacity")?
                    )?)
                .to_string());
                let data = bytes(c["output_data"].as_str().context("Missing cell data")?)?;
                if !token.is_null() {
                    if data.len() == 16 {
                        g["sudt_amount"] = json!((g_num::parse(&g["sudt_amount"])?
                            + num_bigint::BigUint::from_bytes_le(&data))
                        .to_string());
                    } else {
                        g["unexpected_data"] = json!(g["unexpected_data"].as_u64().unwrap() + 1);
                    }
                }
                count += 1;
            }
        }
        ensure!(
            expected["complete_pagination"] == true && expected["cell_count"] == count,
            "Inventory count mismatch"
        );
        let eg = expected["groups"].as_object().context("Missing groups")?;
        ensure!(eg.len() == groups.len(), "Group count mismatch");
        for (key, v) in eg {
            let normalized = if key == "CKB-only" {
                key.clone()
            } else {
                serde_json::from_str::<Value>(key)?.to_string()
            };
            let actual = groups.get(&normalized).context("Missing inventory asset")?;
            for field in [
                "cells",
                "capacity_shannons",
                "sudt_amount",
                "unexpected_data",
            ] {
                ensure!(
                    g_num::parse(&actual[field])? == g_num::parse(&v[field])?,
                    "Inventory arithmetic mismatch"
                );
            }
        }
    }
    println!("PASS: evidence checksums, rollup/config/code hashes, L2 root cross-check and inventory arithmetic. v0 Web3 is one block behind L1 in this saved evidence.");
    Ok(())
}
use std::collections::{BTreeMap, BTreeSet};
mod g_num {
    pub type Big = num_bigint::BigUint;
    pub fn parse(v: &serde_json::Value) -> anyhow::Result<Big> {
        Ok(v.as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| v.to_string())
            .parse()?)
    }
}
