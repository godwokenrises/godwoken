//! Import the complete DB-generated report as a reviewed, root-committed release.
use super::{
    adapter::NativeDb,
    plan::{self, Plan},
};
use crate::model::*;
use anyhow::{ensure, Context, Result};
use exit_withdraw_state_validator::{self as c, settlement::records::*};
use gw_common::state::*;
use gw_types::{prelude::*, registry_address::RegistryAddress, U256};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
fn rows<'a>(v: &'a Value, name: &str) -> Result<&'a Vec<Value>> {
    v[name]
        .as_array()
        .with_context(|| format!("Missing {name}"))
}
fn id(v: &Value) -> Result<u32> {
    Ok(u32::try_from(v.as_u64().context("Integer ID")?)?)
}
fn text(v: &Value) -> Result<&str> {
    v.as_str().context("Expected string")
}
fn n(v: &Value) -> Result<U256> {
    let s = text(v)?;
    ensure!(
        !s.is_empty() && s.bytes().all(|c| c.is_ascii_digit()) && (s == "0" || !s.starts_with('0')),
        "Noncanonical integer"
    );
    U256::from_dec_str(s).map_err(|e| anyhow::anyhow!("{e:?}"))
}
pub fn inspect(
    report: &Value,
    protocol: super::Protocol,
    account: Option<u32>,
    address: Option<&str>,
) -> Result<Value> {
    ensure!(
        report["schema"] == "db-refund-tables-1"
            && report["protocol"] == protocol.name()
            && report["complete_refund_calculation"] == true,
        "Incomplete report or wrong protocol"
    );
    let address = address.map(eth_address).transpose()?;
    let selected = rows(report, "accounts")?
        .iter()
        .filter(|r| {
            account.is_none_or(|a| r["account_id"] == a)
                && address.is_none_or(|a| {
                    r["owner_eth_address"]
                        .as_str()
                        .and_then(|v| eth_address(v).ok())
                        == Some(a)
                })
        })
        .cloned()
        .collect::<Vec<_>>();
    let mut seen = std::collections::BTreeSet::new();
    let mut totals = BTreeMap::<u32, U256>::new();
    for r in &selected {
        ensure!(
            seen.insert((id(&r["account_id"])?, id(&r["asset_id"])?)),
            "Duplicate account/asset row"
        );
        let q = n(&r["payable_l1_raw"])?;
        let t = totals.entry(id(&r["asset_id"])?).or_default();
        *t = t.checked_add(q).context("Amount overflow")?;
    }
    Ok(
        json!({"protocol":protocol,"snapshot_root":report["root"],"accounts":selected,"assets":rows(report,"assets")?,"authorities":rows(report,"authorities")?,"selected_payable_l1_totals":totals.into_iter().map(|(a,n)|json!({"asset_id":a,"raw":n.to_string()})).collect::<Vec<_>>(),"live_paid_state_checked":false}),
    )
}
pub fn export(
    db: &dyn NativeDb,
    report: &Value,
    original: &[u8],
    deployment: c::Hash,
) -> Result<Plan> {
    inspect(report, db.protocol(), None, None)?;
    ensure!(
        report["root"] == db.root(),
        "Report root differs from original DB"
    );
    let native = db.overlay()?;
    let version = db.protocol().native();
    let rollup = hash32(db.protocol().rollup())?;
    let evidence = hash(original);
    let mut recipient_scripts = BTreeMap::new();
    for a in rows(report, "authorities")? {
        let aid = id(&a["account_id"])?;
        if !rows(report, "claim_sources")?
            .iter()
            .any(|r| r["account_id"] == aid)
        {
            continue;
        }
        let mut owners = rows(a, "owners")?
            .iter()
            .map(|o| eth_address(text(&o["eth_address"])?))
            .collect::<Result<Vec<_>>>()?;
        owners.sort();
        let script = c::control::descriptor(
            rollup,
            aid,
            u8::try_from(id(&a["refund_threshold"])?)?,
            &owners,
        )
        .map_err(|e| anyhow::anyhow!("Authority: {e:?}"))?;
        ensure!(
            recipient_scripts
                .insert(aid, Script::from_packed(script)?)
                .is_none(),
            "Duplicate authority"
        );
    }
    let mut assets = BTreeMap::new();
    for a in rows(report, "assets")? {
        let aid = id(&a["asset_id"])?;
        let ty = hash32(text(&a["type_hash"])?)?;
        let (_, script) = db.account(aid)?;
        ensure!(
            bytes(&script.args)? == [rollup.as_slice(), &ty].concat(),
            "Asset Script args differ from report"
        );
        ensure!(
            assets
                .insert(
                    aid,
                    AssetLimit {
                        asset: aid,
                        type_hash: ty,
                        ceiling: n(&a["refund_entitlement_raw"])?
                    }
                )
                .is_none(),
            "Duplicate asset"
        );
    }
    let mut sources = BTreeMap::new();
    let mut lookup = BTreeMap::new();
    let mut lots = BTreeMap::new();
    let mut initial_lots = BTreeMap::new();
    for row in rows(report, "source_reserves")? {
        let aid = id(&row["asset_id"])?;
        let baseline = n(&row["native_input_raw"])?;
        let allocated = n(&row["refund_entitlement_raw"])?;
        let retained = n(&row["retained_raw"])?;
        ensure!(
            allocated.checked_add(retained) == Some(baseline),
            "Source does not conserve assets"
        );
        if baseline.is_zero() {
            continue;
        }
        let (account, sh, address) = if let Some(account) = row["source_account_id"].as_u64() {
            let account = u32::try_from(account)?;
            let (sh, _) = db.account(account)?;
            let address = if db.protocol() == super::Protocol::V0 {
                sh[..20].try_into()?
            } else {
                let key = build_account_key(2, &build_script_hash_to_registry_address_key(&sh));
                let raw = native.read(key)?;
                RegistryAddress::from_slice(&raw)
                    .context("Unregistered source")?
                    .address
                    .as_slice()
                    .try_into()?
            };
            (account, sh, address)
        } else {
            ensure!(
                db.protocol() == super::Protocol::V0 && allocated.is_zero(),
                "Only retained V0 unregistered source accepted"
            );
            (
                c::native::RAW_DESTINATION,
                [0; 32],
                eth_address(text(&row["source_short_address"])?)?,
            )
        };
        version
            .identity(account, sh, address, u32::MAX, |k| {
                native.read(k).map_err(|_| c::Error::Proof)
            })
            .map_err(|e| anyhow::anyhow!("Source identity: {e:?}"))?;
        ensure!(
            version
                .amount(native.read(version.balance_key(aid, address))?)
                .map_err(|e| anyhow::anyhow!("{e:?}"))?
                == baseline,
            "Source balance differs from DB"
        );
        let src = Source {
            account,
            script_hash: sh,
            address,
            asset: aid,
            type_hash: assets.get(&aid).context("Missing asset")?.type_hash,
            baseline,
            evidence,
        };
        let key = source_key(deployment, &src);
        ensure!(
            sources.insert(key, encode(&src)).is_none(),
            "Duplicate source"
        );
        ensure!(
            lookup
                .insert(
                    (row["source_account_id"].to_string(), aid),
                    (key, allocated)
                )
                .is_none(),
            "Duplicate source index"
        );
        for (kind, amount) in [
            (INITIAL, allocated),
            (
                if version.protected(&src) {
                    PROTECTED
                } else {
                    DEFERRED
                },
                retained,
            ),
        ] {
            if amount.is_zero() {
                continue;
            }
            let lot = Lot {
                id: hash(&[b"DB_REPORT_LOT_V1".as_slice(), &key, &[kind]].concat()),
                source: key,
                kind,
                amount,
                evidence,
                reopen: [0; 32],
            };
            let lk = lot_key(deployment, &lot);
            if kind == INITIAL {
                initial_lots.insert(key, lk);
            }
            lots.insert(lk, encode(&lot));
        }
    }
    let mut sums = BTreeMap::<(c::Hash, u32), U256>::new();
    for row in rows(report, "claim_sources")? {
        let aid = id(&row["asset_id"])?;
        let owner = id(&row["account_id"])?;
        let amount = n(&row["entitlement_raw"])?;
        if amount.is_zero() {
            continue;
        }
        let source = lookup
            .get(&(row["source_account_id"].to_string(), aid))
            .context("Claim source absent")?
            .0;
        let sum = sums.entry((source, owner)).or_default();
        *sum = sum.checked_add(amount).context("Overflow")?;
    }
    let mut claims = BTreeMap::new();
    let mut totals = BTreeMap::<(u32, u32), U256>::new();
    for ((source, owner), cap) in sums {
        let src: Source = decode(&sources[&source]).map_err(|e| anyhow::anyhow!("{e:?}"))?;
        let (script, address) = if let Some(s) = recipient_scripts.get(&owner) {
            let s = s.packed()?;
            let a = c::control::identity(&s);
            (s, a)
        } else {
            let (_, s) = db.account(owner)?;
            let args = bytes(&s.args)?;
            ensure!(
                args.len() == 52 && args[..32] == rollup,
                "Beneficiary is not ETH EOA"
            );
            let a: [u8; 20] = args[32..].try_into()?;
            let info = db.lookup_eth(&a)?;
            ensure!(
                info["status"] == "registered" && info["account_id"] == owner,
                "Beneficiary ETH mapping mismatch"
            );
            (s.packed()?, a)
        };
        let lot = *initial_lots.get(&source).context("Initial lot absent")?;
        let claim = Claim {
            epoch: 0,
            source,
            lot,
            recipient: owner,
            script_hash: c::script_hash(&script),
            address,
            cap,
            evidence,
        };
        ensure!(
            claims
                .insert(claim_key(deployment, &claim), encode(&claim))
                .is_none(),
            "Duplicate claim"
        );
        let total = totals.entry((owner, src.asset)).or_default();
        *total = total.checked_add(cap).context("Overflow")?;
    }
    for row in rows(report, "accounts")? {
        let key = (id(&row["account_id"])?, id(&row["asset_id"])?);
        ensure!(
            totals.remove(&key).unwrap_or_default() == n(&row["entitlement_raw"])?,
            "Account table differs from source attribution"
        );
    }
    ensure!(totals.is_empty(), "Missing account rows");
    assets.retain(|id, _| {
        sources
            .values()
            .any(|b| decode::<Source>(b).is_ok_and(|s| s.asset == *id))
    });
    let mut p = Plan {
        protocol: db.protocol(),
        version: 2,
        deployment: hx(deployment),
        snapshot_root: db.root(),
        manifest: String::new(),
        policy: hx(hash(text(&report["policy_sha256"])?.as_bytes())),
        sources: sources.values().map(hx).collect(),
        reserves: lots.values().map(hx).collect(),
        claims: claims.values().map(hx).collect(),
        limits: assets.values().map(|v| hx(encode(v))).collect(),
        source_root: plan::root(&sources)?,
        reserve_root: plan::root(&lots)?,
        claims_root: plan::root(&claims)?,
        input_sha256: BTreeMap::from([(
            "report.json".into(),
            hex::encode(Sha256::digest(original)),
        )]),
        recipient_scripts,
    };
    p.manifest = p.compute_manifest()?;
    p.verify()?;
    Ok(p)
}
