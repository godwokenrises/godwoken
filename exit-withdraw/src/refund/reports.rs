//! Deterministic offline report serialization. L1 custody is deliberately absent.
use anyhow::{ensure, Context, Result};
use num_bigint::BigUint as B;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
fn array(v: &Value) -> Result<&Vec<Value>> {
    v.as_array().context("Report array")
}
fn id(v: &Value) -> Result<u64> {
    v.as_u64().context("Report id")
}
fn text(v: &Value) -> Result<&str> {
    v.as_str().context("Report string")
}
fn n(v: &Value) -> Result<B> {
    Ok(text(v)?.parse()?)
}
fn display(n: &B, d: usize) -> String {
    if d == 0 {
        return n.to_string();
    }
    let s = format!("{:0>width$}", n, width = d + 1);
    let s = format!("{}.{}", &s[..s.len() - d], &s[s.len() - d..]);
    s.trim_end_matches('0').trim_end_matches('.').into()
}
pub fn v0(candidate: &Value) -> Result<Value> {
    ensure!(candidate["protocol"] == "v0", "Report version");
    let assets = array(&candidate["asset_metadata"])?
        .iter()
        .map(|a| Ok((id(&a["account_id"])?, a)))
        .collect::<Result<BTreeMap<_, _>>>()?;
    let identities = array(&candidate["identities"])?
        .iter()
        .map(|a| Ok((id(&a["account_id"])?, a)))
        .collect::<Result<BTreeMap<_, _>>>()?;
    let capacity = array(&candidate["capacity"])?
        .iter()
        .map(|a| Ok((id(&a["account_id"])?, a)))
        .collect::<Result<BTreeMap<_, _>>>()?;
    let authorities = array(&candidate["authorities"])?
        .iter()
        .map(|a| Ok((id(&a["account_id"])?, a)))
        .collect::<Result<BTreeMap<_, _>>>()?;
    let mut paths = BTreeMap::<(u64, u64), BTreeSet<&str>>::new();
    let mut claims = vec![];
    for claim in array(&candidate["claims"])? {
        let owner = id(&claim["account_id"])?;
        let asset = id(&claim["asset_id"])?;
        let path = text(&claim["protocol_path"])?;
        paths.entry((owner, asset)).or_default().insert(path);
        let status = if !authorities.contains_key(&owner) {
            "INCLUDED_EOA"
        } else if path.starts_with("approved pending protocol fee") {
            "INCLUDE_APPROVED_V0_PROTOCOL_FEE"
        } else {
            "INCLUDE_EXISTING_CONTROL_RIGHT"
        };
        claims.push(json!({"account_id":owner,"owner_eth_address":claim["owner_eth_address"],"asset_id":asset,"entitlement_raw":claim["entitlement_raw"],"source_account_id":claim["source_account_id"],"protocol_path":path,"disposition":status}));
    }
    let mut rows = vec![];
    let mut capacities = vec![];
    for row in array(&candidate["accounts"])? {
        let owner = id(&row["account_id"])?;
        let aid = id(&row["asset_id"])?;
        let asset = assets.get(&aid).context("Unknown asset")?;
        let ident = identities.get(&owner).context("Unknown beneficiary")?;
        let c = capacity.get(&owner).context("Missing capacity")?;
        let raw = n(&row["entitlement_raw"])?;
        ensure!(raw.bits() <= 128, "Amount exceeds sUDT output width");
        let amount = display(&raw, id(&asset["decimals"])? as usize);
        let ready = n(&c["topup_shannons"])? == B::default();
        let control = authorities.contains_key(&owner);
        let status = match (ready, control) {
            (true, false) => "CAPACITY_READY",
            (false, false) => "NEEDS_CKB_TOPUP",
            (true, true) => "CAPACITY_READY_IMPLEMENTATION_PENDING",
            (false, true) => "NEEDS_CKB_TOPUP_IMPLEMENTATION_PENDING",
        };
        rows.push(json!({"account_id":owner,"account_kind":if control{"CONTRACT"}else{"EOA"},"owner_eth_address":ident["owner_eth_address"],"refund_authority":if control{"2-of-3 distinct authenticated owners"}else{"ETH owner"},"asset_id":aid,"symbol":asset["symbol"],"type_hash":asset["type_hash"],"l2_decimals":asset["decimals"],"entitlement_raw":raw.to_string(),"entitlement":amount,"payable_l2_raw":raw.to_string(),"payable":amount,"capacity_ready_l2_raw":if ready{raw.to_string()}else{"0".into()},"capacity_ready":if ready{amount.as_str()}else{"0"},"rounding_retained":"0","status":status,"protocols_or_reason":paths[&(owner,aid)].iter().copied().collect::<Vec<_>>().join("; "),"account_topup_shannons_do_not_sum_per_asset":c["topup_shannons"],"payable_l1_raw":raw.to_string(),"capacity_ready_l1_raw":if ready{raw.to_string()}else{"0".into()}}));
    }
    // Preserve the explicitly held treasury in the account register, without a claim.
    for source in array(&candidate["sources"])?
        .iter()
        .filter(|r| r["source_account_id"] == 36838)
    {
        let aid = id(&source["asset_id"])?;
        let asset = assets[&aid];
        rows.push(json!({"account_id":36838,"account_kind":"CONTRACT","owner_eth_address":null,"refund_authority":"Not a multisig; separate business disposition","asset_id":aid,"symbol":asset["symbol"],"type_hash":asset["type_hash"],"l2_decimals":asset["decimals"],"entitlement_raw":"0","entitlement":"0","payable_l2_raw":"0","payable":"0","capacity_ready_l2_raw":"0","capacity_ready":"0","rounding_retained":"0","status":"HELD_NON_MULTISIG; separate decision required","protocols_or_reason":"No refund; recorded holdings and reason: HELD_NON_MULTISIG; separate decision required","account_topup_shannons_do_not_sum_per_asset":null,"payable_l1_raw":"0","capacity_ready_l1_raw":"0"}));
    }
    rows.sort_by_key(|r| {
        (
            r["account_id"].as_u64().unwrap(),
            r["asset_id"].as_u64().unwrap(),
        )
    });
    for (owner, c) in &capacity {
        let mut c = (*c).clone();
        c["account_kind"] = json!(if authorities.contains_key(owner) {
            "CONTRACT"
        } else {
            "EOA"
        });
        c["topup_ckb"] = json!(display(&n(&c["topup_shannons"])?, 8));
        capacities.push(c);
    }
    let mut asset_rows = vec![];
    for (aid, a) in &assets {
        let sources = array(&candidate["sources"])?
            .iter()
            .filter(|r| r["asset_id"] == *aid)
            .collect::<Vec<_>>();
        let start = sources
            .iter()
            .map(|r| n(&r["native_input_raw"]))
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .sum::<B>();
        let refund = sources
            .iter()
            .map(|r| n(&r["refund_entitlement_raw"]))
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .sum::<B>();
        let retained = sources
            .iter()
            .map(|r| n(&r["retained_raw"]))
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .sum::<B>();
        ensure!(start == &refund + &retained, "Asset source conservation");
        asset_rows.push(json!({"asset_id":aid,"symbol":a["symbol"],"type_hash":a["type_hash"],"decimals":a["decimals"],"discovered_l2_before_raw":start.to_string(),"refund_entitlement_raw":refund.to_string(),"discovered_l2_retained_raw":retained.to_string(),"conservation_pass":true}));
    }
    Ok(
        json!({"schema":"db-refund-tables-1","protocol":"v0","root":candidate["root"],"policy_sha256":candidate["policy_sha256"],"accounts":rows,"capacity":capacities,"claim_sources":claims,"source_reserves":candidate["sources"],"assets":asset_rows,"authorities":candidate["authorities"],"custodian_observation":null,"complete_refund_calculation":true,"execution_ready":false,"capacity_assumption":candidate["capacity_assumption"]}),
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_display_preserves_small_units_and_large_integers() {
        assert_eq!(display(&B::from(1u8), 18), "0.000000000000000001");
        assert_eq!(display(&B::default(), 8), "0");
        assert_eq!(display(&B::from(100000000u64), 8), "1");
        assert_eq!(
            display(&(B::from(1u8) << 128usize), 0),
            "340282366920938463463374607431768211456"
        );
    }
}

/// Publish only after the calculation succeeds. A failed write leaves no success manifest.
pub fn write_tables(output: &std::path::Path, tables: &Value) -> Result<()> {
    use std::{fs, io::Write};
    ensure!(!output.exists(), "Report output already exists");
    fs::create_dir(output)?;
    for (key, name) in [
        ("accounts", "accounts.csv"),
        ("capacity", "capacity.csv"),
        ("claim_sources", "claim-sources.csv"),
        ("source_reserves", "source-reserves.csv"),
        ("assets", "assets.csv"),
    ] {
        let rows = array(&tables[key])?;
        let fields = rows
            .iter()
            .map(|r| r.as_object().context("Table object"))
            .collect::<Result<Vec<_>>>()?
            .iter()
            .flat_map(|r| r.keys().cloned())
            .collect::<BTreeSet<_>>();
        let mut writer = csv::Writer::from_writer(
            fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(output.join(name))?,
        );
        writer.write_record(&fields)?;
        for row in rows {
            writer.write_record(fields.iter().map(|k| match &row[k] {
                Value::Null => String::new(),
                Value::String(s) => s.clone(),
                v => v.to_string(),
            }))?;
        }
        writer.flush()?;
    }
    for (name, data) in [
        ("authorities.json", &tables["authorities"]),
        ("report.json", tables),
    ] {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(output.join(name))?;
        file.write_all(&serde_json::to_vec_pretty(data)?)?;
    }
    if let Some(lots) = tables.get("retained_lots") {
        crate::report::write_new(
            &output.join("retained-lots.json"),
            &serde_json::to_vec_pretty(lots)?,
        )?;
    }
    use sha2::{Digest, Sha256};
    let mut entries = Vec::new();
    for name in [
        "accounts.csv",
        "capacity.csv",
        "claim-sources.csv",
        "source-reserves.csv",
        "assets.csv",
        "authorities.json",
        "report.json",
    ] {
        entries.push(format!(
            "{}  {}\n",
            hex::encode(Sha256::digest(fs::read(output.join(name))?)),
            name
        ));
    }
    if tables.get("retained_lots").is_some() {
        entries.push(format!(
            "{}  retained-lots.json\n",
            hex::encode(Sha256::digest(fs::read(output.join("retained-lots.json"))?))
        ));
    }
    let mut manifest = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output.join("SHA256SUMS"))?;
    manifest.write_all(entries.concat().as_bytes())?;
    Ok(())
}
