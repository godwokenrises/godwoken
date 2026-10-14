//! Authenticated non-EOA account inventory, without interpreting protocol positions.
use crate::{audit, db::Database, model::*, report};
use anyhow::{ensure, Context, Result};
use gw_common::state::State;
use gw_types::{prelude::*, U256};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

pub fn account_kind(script: &Script, cfg: &Config) -> Result<&'static str> {
    if script.hash_type != "type" {
        return Ok("unknown");
    }
    let code = hash32(&script.code_hash)?;
    let args = bytes(&script.args)?;
    for allowed in cfg.packed.allowed_eoa_type_hashes().into_iter() {
        if allowed.hash().as_slice() == code {
            return Ok(if u8::from(allowed.type_()) == 1 && args.len() == 52 {
                "eth_eoa"
            } else {
                "other_eoa"
            });
        }
    }
    for allowed in cfg.packed.allowed_contract_type_hashes().into_iter() {
        if allowed.hash().as_slice() == code {
            return Ok(match u8::from(allowed.type_()) {
                1 => "system_meta",
                2 => "system_sudt",
                3 => "polyjuice",
                4 => "system_registry",
                _ => "unknown",
            });
        }
    }
    Ok("unknown")
}
fn decimal(raw: &str, decimals: Option<u64>) -> Option<String> {
    let d = usize::try_from(decimals?).ok()?;
    if d > 80 {
        return None;
    }
    if d == 0 {
        return Some(raw.into());
    }
    let s = format!("{:0>width$}", raw, width = d + 1);
    Some(format!("{}.{}", &s[..s.len() - d], &s[s.len() - d..]))
}
fn csv(path: &Path, rows: &[Value], fields: &[&str]) -> Result<()> {
    let mut w = csv::Writer::from_writer(Vec::new());
    w.write_record(fields)?;
    for row in rows {
        w.write_record(fields.iter().map(|k| match &row[*k] {
            Value::Null => String::new(),
            Value::String(s) => s.clone(),
            v => v.to_string(),
        }))?;
    }
    report::write_new(path, &w.into_inner().map_err(|e| e.into_error())?)
}
pub fn run(db: &Database, bundle: &Bundle, output: &Path) -> Result<Value> {
    let audit = audit::run(db, bundle, None, false)?;
    ensure!(
        audit["checks"]
            .as_array()
            .context("Missing checks")?
            .iter()
            .all(|x| x["status"] == "PASS"),
        "State/root checks failed"
    );
    let (_, cfg) = bundle.validate()?;
    let catalogue = audit["assets"].as_array().context("Missing assets")?;
    let assets: Vec<_> = catalogue
        .iter()
        .filter(|a| a["l2_account_id"].is_u64())
        .collect();
    let empty_code_hash = hash(&[]);
    let mut accounts = Vec::new();
    let mut balances = Vec::new();
    let mut counts = BTreeMap::<String, u32>::new();
    let mut groups = BTreeMap::<String, Vec<u32>>::new();
    let mut code_blobs = BTreeMap::<String, Vec<u8>>::new();
    let mut sums = BTreeMap::<u32, (U256, U256, u32)>::new();
    let mut unresolved = Vec::new();
    let mut missing_codes = BTreeSet::new();
    let mut missing_code_accounts = Vec::new();
    let mut funded = 0;
    let mut funded_evm = 0;
    for id in 0..db.tip.account_count {
        let (sh, script) = db.account(id)?;
        let args = bytes(&script.args)?;
        ensure!(args.starts_with(&db.chain_id), "Wrong rollup prefix");
        let kind = account_kind(&script, &cfg)?;
        if kind == "eth_eoa" || kind == "other_eoa" {
            *counts.entry(kind.into()).or_default() += 1;
            continue;
        }
        let registry = db.registry(&sh)?;
        let address = registry.as_ref().map(|r| hx(&r.address));
        let mut runtime_hash = None;
        let mut runtime_size = None;
        let mut code_status = "not_applicable";
        let mut destruction = None;
        let mut classification = kind;
        if kind == "polyjuice" {
            let mut key = [0u8; 32];
            key[..4].copy_from_slice(&id.to_le_bytes());
            key[4] = 0xff;
            key[5] = 1;
            let h = db.get_value(id, &key)?;
            key[5] = 2;
            let flag = db.get_value(id, &key)?;
            destruction = Some(hx(flag));
            classification = if flag.iter().any(|x| *x != 0) {
                "polyjuice_destructed"
            } else if h == [0; 32] {
                "polyjuice_no_code"
            } else if h == empty_code_hash {
                "polyjuice_empty_code"
            } else {
                "polyjuice_runtime"
            };
            code_status = "no_code_reference";
            if h != [0; 32] {
                let hash = hx(h);
                if !code_blobs.contains_key(&hash) && !missing_codes.contains(&hash) {
                    // Empty code has a known verifiable preimage even when no DB blob is stored.
                    let code = db
                        .data(&h)?
                        .or_else(|| (h == empty_code_hash).then(Vec::new));
                    if let Some(code) = code {
                        code_blobs.insert(hash.clone(), code);
                    } else {
                        missing_codes.insert(hash.clone());
                    }
                }
                runtime_size = code_blobs.get(&hash).map(Vec::len);
                code_status = if h == empty_code_hash {
                    "verified_empty_preimage"
                } else if runtime_size.is_some() {
                    "verified"
                } else {
                    "missing_preimage"
                };
                if runtime_size.is_none() {
                    missing_code_accounts.push(id);
                    eprintln!("Missing runtime code preimage for account {id}: {hash}");
                }
                groups.entry(hash.clone()).or_default().push(id);
                runtime_hash = Some(hash);
            }
        }
        *counts.entry(classification.into()).or_default() += 1;
        let mut positive = 0;
        let mut tokens = 0;
        let mut ckb = None;
        if let Some(ref reg) = registry {
            for asset in &assets {
                let aid = u32::try_from(asset["l2_account_id"].as_u64().unwrap())?;
                let n = db.get_sudt_balance(aid, reg)?;
                let sum = sums.entry(aid).or_default();
                sum.0 = sum.0.checked_add(n).context("Balance sum overflow")?;
                if kind == "polyjuice" {
                    sum.1 = sum.1.checked_add(n).context("EVM balance sum overflow")?;
                }
                if !n.is_zero() {
                    positive += 1;
                    sum.2 += 1;
                    if aid != 1 {
                        tokens += 1;
                    }
                }
                let d = if aid == 1 {
                    Some(18)
                } else {
                    asset["decimals"].as_u64()
                };
                let display = decimal(&n.to_string(), d);
                if aid == 1 {
                    ckb = display.clone();
                }
                balances.push(json!({"account_id":id,"eth_address":address,"classification":classification,"asset_id":aid,"asset_type_hash":asset["asset_id"],"symbol":asset["symbol"],"decimals":d,"balance_l2_raw":n.to_string(),"display":display,"is_positive":!n.is_zero()}));
            }
        } else {
            unresolved.push(id);
        }
        if positive > 0 {
            funded += 1;
            if kind == "polyjuice" {
                funded_evm += 1;
            }
        }
        accounts.push(json!({"account_id":id,"eth_address":address,"classification":classification,"script":script,"script_hash":hx(sh),"runtime_code_hash":runtime_hash,"runtime_bytes":runtime_size,"code_preimage_status":code_status,"destructed_raw":destruction,"balance_status":if registry.is_some(){"authenticated"}else{"unresolved_no_registry"},"positive_backed_asset_count":if registry.is_some(){Some(positive)}else{None},"positive_backed_token_count":if registry.is_some(){Some(tokens)}else{None},"ckb_display":ckb,"ordinary_erc20_lp_status":"not_scanned"}));
        if accounts.len() % 100 == 0 {
            eprintln!("Inventoried {} non-EOA accounts", accounts.len());
        }
    }
    let totals:Vec<_>=assets.iter().map(|a|{let id=a["l2_account_id"].as_u64().unwrap() as u32;let (total,evm,holders)=sums.get(&id).copied().unwrap_or_default();let d=if id==1 {Some(18)}else{a["decimals"].as_u64()};json!({"asset_id":id,"symbol":a["symbol"],"asset_type_hash":a["asset_id"],"non_eoa_balance_raw":total.to_string(),"non_eoa_display":decimal(&total.to_string(),d),"polyjuice_balance_raw":evm.to_string(),"polyjuice_display":decimal(&evm.to_string(),d),"positive_non_eoa_holders":holders,"l2_total_supply_raw":a["l2_total_supply_raw"]})}).collect();
    let code_groups:Vec<_>=groups.iter().map(|(h,ids)|json!({"runtime_code_hash":h,"account_count":ids.len(),"account_ids":ids,"runtime_bytes":code_blobs.get(h).map(Vec::len),"code_preimage_status":if code_blobs.contains_key(h){"verified"}else{"missing_preimage"}})).collect();
    let summary = json!({"schema_version":1,"generated_at":now(),"scope":"Mainnet v1 original DB and saved L1 snapshot. Non-EOA account identities and native CKB/L2 sUDT balances. Ordinary EVM ERC20/LP holdings and protocol roles have NOT been scanned.","state_root":db.root(),"l2_tip":db.tip,"total_accounts":db.tip.account_count,"counts":counts,"non_eoa_accounts":accounts.len(),"non_eoa_accounts_with_backed_assets":funded,"polyjuice_accounts_with_backed_assets":funded_evm,"non_eoa_accounts_without_registry":unresolved,"backed_asset_count":assets.len(),"balance_rows":balances.len(),"distinct_runtime_code_hashes":groups.len(),"missing_runtime_code_hashes":missing_codes,"accounts_with_missing_runtime_code":missing_code_accounts,"runtime_code_hash_algorithm":"CKB-personalized Blake2b-256","code_evidence_complete":missing_codes.is_empty(),"authenticated_reads":db.checked_reads.get(),"state_checks":audit["checks"],"backing_audit_status":audit["overall_status"],"limitations":["EOA means account script type, not verified individual beneficial ownership.","Non-EOA includes system accounts and unknown scripts; use classification rather than treating them all as application contracts.","Missing registry means unresolved balances, never proven zero.","Runtime-code grouping is exact byte identity, not protocol identification; proxy implementations and immutable arguments are not resolved.","Generic ERC20/LP tokens can be stored in EVM storage and are not covered by this backed-token catalogue.","This original snapshot does not include simulated withdrawal state changes."]});
    ensure!(
        counts.values().sum::<u32>() == db.tip.account_count,
        "Incomplete account classification"
    );
    fs::create_dir(output)?;
    fs::create_dir(output.join("runtime-code"))?;
    for (h, code) in code_blobs {
        report::write_new(
            &output.join("runtime-code").join(format!("{}.bin", h)),
            &code,
        )?;
    }
    for (name, data) in [("summary.json", &summary), ("audit.json", &audit)] {
        report::write_new(&output.join(name), &serde_json::to_vec_pretty(data)?)?;
    }
    for (name, rows) in [
        ("accounts.json", &accounts),
        ("balances.json", &balances),
        ("asset-totals.json", &totals),
        ("code-groups.json", &code_groups),
    ] {
        report::write_new(&output.join(name), &serde_json::to_vec_pretty(rows)?)?;
    }
    csv(
        &output.join("accounts.csv"),
        &accounts,
        &[
            "account_id",
            "eth_address",
            "classification",
            "runtime_code_hash",
            "runtime_bytes",
            "code_preimage_status",
            "balance_status",
            "positive_backed_asset_count",
            "positive_backed_token_count",
            "ckb_display",
            "ordinary_erc20_lp_status",
        ],
    )?;
    csv(
        &output.join("balances.csv"),
        &balances,
        &[
            "account_id",
            "eth_address",
            "classification",
            "asset_id",
            "symbol",
            "asset_type_hash",
            "balance_l2_raw",
            "decimals",
            "display",
            "is_positive",
        ],
    )?;
    csv(
        &output.join("asset-totals.csv"),
        &totals,
        &[
            "asset_id",
            "symbol",
            "asset_type_hash",
            "non_eoa_balance_raw",
            "non_eoa_display",
            "polyjuice_balance_raw",
            "polyjuice_display",
            "positive_non_eoa_holders",
            "l2_total_supply_raw",
        ],
    )?;
    let mut files = Vec::new();
    for e in fs::read_dir(output)? {
        let p = e?.path();
        if p.is_file() {
            files.push(p);
        } else {
            for e in fs::read_dir(p)? {
                files.push(e?.path());
            }
        }
    }
    files.sort();
    use sha2::{Digest, Sha256};
    let hashes = files
        .iter()
        .map(|p| {
            Ok(format!(
                "{}  {}\n",
                hex::encode(Sha256::digest(fs::read(p)?)),
                p.strip_prefix(output)?.display()
            ))
        })
        .collect::<Result<String>>()?;
    report::write_new(&output.join("SHA256SUMS"), hashes.as_bytes())?;
    Ok(summary)
}
