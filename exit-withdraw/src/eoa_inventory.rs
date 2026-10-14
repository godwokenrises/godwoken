//! Authenticated EOA balances, preserving unresolved registries and exact integers.
use crate::{audit, contract_inventory::account_kind, db::Database, model::*, report::write_new};
use anyhow::{ensure, Context, Result};
use gw_common::state::State;
use gw_types::U256;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

pub fn decimal(raw: &str, decimals: Option<u64>) -> Option<String> {
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
fn csv(out: &Path, name: &str, rows: &[Value], fields: &[&str]) -> Result<()> {
    let mut w = csv::Writer::from_writer(Vec::new());
    w.write_record(fields)?;
    for r in rows {
        w.write_record(fields.iter().map(|f| {
            r[*f].as_str().map(str::to_owned).unwrap_or_else(|| {
                if r[*f].is_null() {
                    String::new()
                } else {
                    r[*f].to_string()
                }
            })
        }))?;
    }
    write_new(
        &out.join(name),
        &w.into_inner().map_err(|e| e.into_error())?,
    )
}
pub fn run(db: &Database, bundle: &Bundle, out: &Path) -> Result<Value> {
    ensure!(!out.exists(), "Output already exists");
    let parent = out.parent().context("output parent")?.canonicalize()?;
    ensure!(!parent.starts_with(&db.path), "Output inside DB");
    let audit = audit::run(db, bundle, None, false)?;
    ensure!(
        audit["checks"]
            .as_array()
            .context("checks")?
            .iter()
            .all(|c| c["status"] == "PASS"),
        "State/root checks failed"
    );
    let (_, cfg) = bundle.validate()?;
    let assets: Vec<_> = audit["assets"]
        .as_array()
        .context("assets")?
        .iter()
        .filter(|r| r["l2_account_id"].is_u64())
        .collect();
    let mut accounts = Vec::new();
    let mut positives = Vec::new();
    let mut totals = BTreeMap::<u32, (U256, u64)>::new();
    let mut seen = BTreeSet::new();
    let mut unresolved = Vec::new();
    let mut authenticated = 0u64;
    let mut funded = 0u64;
    let mut query_count = 0u64;
    let selected: Vec<_> = assets
        .iter()
        .filter(|a| {
            a["buckets"]["finalized_custodian"]["cells"]
                .as_u64()
                .unwrap_or(0)
                + a["buckets"]["unfinalized_custodian"]["cells"]
                    .as_u64()
                    .unwrap_or(0)
                > 0
        })
        .collect();
    let mut wide = csv::Writer::from_writer(Vec::new());
    let mut header = vec![
        "account_id".to_owned(),
        "eth_address".into(),
        "balance_status".into(),
    ];
    header.extend(selected.iter().map(|a| {
        format!(
            "asset_{}_{}_display",
            a["l2_account_id"],
            a["symbol"].as_str().unwrap_or("unlabeled")
        )
    }));
    wide.write_record(&header)?;
    for id in 0..db.tip.account_count {
        let (sh, script) = db.account(id)?;
        let kind = account_kind(&script, &cfg)?;
        if kind != "eth_eoa" && kind != "other_eoa" {
            continue;
        }
        let args = bytes(&script.args)?;
        ensure!(args.starts_with(&db.chain_id), "Wrong rollup prefix");
        let registry = db.registry(&sh)?;
        let address = registry.as_ref().map(|r| hx(&r.address));
        let status = if registry.is_some() {
            "authenticated"
        } else {
            "unresolved_no_registry"
        };
        let mut own = BTreeMap::<u32, (String, Option<String>)>::new();
        let mut positive = 0;
        let mut non_ckb = 0;
        if let Some(ref reg) = registry {
            ensure!(seen.insert(reg.address.clone()), "Duplicate EOA registry");
            if kind == "eth_eoa" {
                ensure!(
                    &args[32..] == reg.address.as_slice(),
                    "EOA address/registry mismatch"
                );
            }
            authenticated += 1;
            for a in &assets {
                let aid = u32::try_from(a["l2_account_id"].as_u64().unwrap())?;
                let n = db.get_sudt_balance(aid, reg)?;
                query_count += 1;
                let raw = n.to_string();
                let decimals = if aid == 1 {
                    Some(18)
                } else {
                    a["decimals"].as_u64()
                };
                let display = decimal(&raw, decimals);
                let t = totals.entry(aid).or_default();
                t.0 = t.0.checked_add(n).context("EOA balance total overflow")?;
                if !n.is_zero() {
                    positive += 1;
                    t.1 += 1;
                    if aid != 1 {
                        non_ckb += 1;
                    }
                    positives.push(json!({"account_id":id,"eth_address":address,"classification":kind,"asset_id":aid,"asset_type_hash":a["asset_id"],"symbol":a["symbol"],"decimals":decimals,"balance_l2_raw":raw,"display":display}));
                }
                own.insert(aid, (raw, display));
            }
        } else {
            unresolved.push(id);
        }
        funded += u64::from(positive > 0);
        let mut wide_row = vec![
            id.to_string(),
            address.clone().unwrap_or_default(),
            status.into(),
        ];
        wide_row.extend(selected.iter().map(|a| {
            own.get(&(a["l2_account_id"].as_u64().unwrap() as u32))
                .and_then(|v| v.1.clone())
                .unwrap_or_default()
        }));
        wide.write_record(wide_row)?;
        accounts.push(json!({"account_id":id,"eth_address":address,"classification":kind,"script_hash":hx(sh),"balance_status":status,"checked_asset_count":if registry.is_some(){assets.len()}else{0},"positive_asset_count":if registry.is_some(){Some(positive)}else{None},"positive_non_ckb_count":if registry.is_some(){Some(non_ckb)}else{None},"ckb_display":own.get(&1).and_then(|v|v.1.clone()),"ordinary_erc20_lp_status":"not_scanned"}));
        if accounts.len() % 1000 == 0 {
            eprintln!(
                "Authenticated {} EOA accounts / {} balance reads",
                accounts.len(),
                query_count
            );
        }
    }
    ensure!(
        query_count == authenticated * assets.len() as u64,
        "Incomplete EOA balance scan"
    );
    let totals:Vec<_>=assets.iter().map(|a|{let id=a["l2_account_id"].as_u64().unwrap() as u32;let (n,holders)=totals.get(&id).copied().unwrap_or_default();let d=if id==1{Some(18)}else{a["decimals"].as_u64()};json!({"asset_id":id,"asset_type_hash":a["asset_id"],"symbol":a["symbol"],"decimals":d,"eoa_balance_raw":n.to_string(),"eoa_display":decimal(&n.to_string(),d),"positive_eoa_holders":holders,"l2_total_supply_raw":a["l2_total_supply_raw"]})}).collect();
    let summary = json!({"state_root":db.root(),"l2_tip":db.tip,"eoa_accounts":accounts.len(),"authenticated_eoa_accounts":authenticated,"funded_eoa_accounts":funded,"unresolved_registry_accounts":unresolved,"assets_checked_per_registered_eoa":assets.len(),"custodian_asset_count":selected.len(),"balance_queries":query_count,"positive_balance_rows":positives.len(),"authenticated_reads":db.checked_reads.get(),"state_checks":audit["checks"],"backing_audit_status":audit["overall_status"],"limitations":["EOA denotes the account Script class, not verified natural-person ownership.","All registered EOA x L2 asset balances were authenticated. Positive balances are exported; checked zeros are implicit only for authenticated accounts.","Ordinary EVM ERC20/LP/share/debt/NFT positions are not scanned.","Custodian CKB uses shannons; L2 CKB balances use 18-decimal units.","This is the original committed snapshot, not fresh mainnet liveness or an approved payout list."],"source_sha256":hex::encode(Sha256::digest(include_bytes!("eoa_inventory.rs")))});
    fs::create_dir(out)?;
    for (name, value) in [
        ("summary.json", summary.clone()),
        ("audit.json", audit),
        ("accounts.json", json!(accounts)),
        ("positive-balances.json", json!(positives)),
        ("asset-totals.json", json!(totals)),
    ] {
        write_new(&out.join(name), &serde_json::to_vec_pretty(&value)?)?;
    }
    csv(
        out,
        "accounts.csv",
        &accounts,
        &[
            "account_id",
            "eth_address",
            "classification",
            "balance_status",
            "checked_asset_count",
            "positive_asset_count",
            "positive_non_ckb_count",
            "ckb_display",
            "ordinary_erc20_lp_status",
        ],
    )?;
    csv(
        out,
        "positive-balances.csv",
        &positives,
        &[
            "account_id",
            "eth_address",
            "asset_id",
            "asset_type_hash",
            "symbol",
            "balance_l2_raw",
            "decimals",
            "display",
        ],
    )?;
    csv(
        out,
        "asset-totals.csv",
        &totals,
        &[
            "asset_id",
            "asset_type_hash",
            "symbol",
            "decimals",
            "eoa_balance_raw",
            "eoa_display",
            "positive_eoa_holders",
            "l2_total_supply_raw",
        ],
    )?;
    write_new(
        &out.join("custodian-balances-wide.csv"),
        &wide.into_inner().map_err(|e| e.into_error())?,
    )?;
    let mut files = fs::read_dir(out)?
        .map(|e| Ok(e?.path()))
        .collect::<Result<Vec<_>>>()?;
    files.sort();
    let mut sums = String::new();
    for p in files {
        sums += &format!(
            "{}  {}\n",
            hex::encode(Sha256::digest(fs::read(&p)?)),
            p.file_name().unwrap().to_string_lossy()
        );
    }
    write_new(&out.join("SHA256SUMS"), sums.as_bytes())?;
    Ok(summary)
}
