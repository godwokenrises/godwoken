//! Independent L2 state and L1 custody queries. Never calculate refund entitlements.
use crate::{
    audit,
    db::Database,
    model::*,
    settlement::adapter::{NativeDb, Protocol},
    v0,
};
use anyhow::{ensure, Context, Result};
use gw_common::state::State;
use gw_types::{prelude::*, registry_address::RegistryAddress};
use num_bigint::{BigInt, BigUint};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

pub fn l2_db(protocol: Protocol, path: &Path, eth: Option<[u8; 20]>) -> Result<Value> {
    match protocol {
        Protocol::V1 => v1_db(path, eth),
        Protocol::V0 => v0_db(path, eth),
    }
}
fn v1_db(path: &Path, eth: Option<[u8; 20]>) -> Result<Value> {
    let db = Database::open(path)?;
    ensure!(
        db.root() == db.tip.account_root,
        "DB root differs from last-valid block"
    );
    let (_, native) = db.account(1)?;
    ensure!(
        native.hash_type == "type"
            && native.args == hx([db.chain_id.as_slice(), &[0; 32]].concat()),
        "Invalid native CKB account"
    );
    let mut accounts = vec![];
    let mut assets = vec![];
    let mut registrations = vec![];
    let mut seen = BTreeSet::new();
    let mut asset_types = BTreeSet::new();
    for id in 0..db.tip.account_count {
        let (sh, script) = db.account(id)?;
        let args = bytes(&script.args)?;
        ensure!(args.starts_with(&db.chain_id), "Foreign account {id}");
        if let Some(reg) = db.registry(&sh)? {
            ensure!(seen.insert(reg.to_bytes()), "Duplicate registration");
            registrations.push((id, reg));
        }
        accounts.push(json!({"account_id":id,"script_hash":hx(sh),"script":script}));
        if script.code_hash == native.code_hash && script.hash_type == "type" {
            ensure!(args.len() == 64, "Invalid native asset args");
            let th = hx(&args[32..]);
            ensure!(asset_types.insert(th.clone()), "Duplicate native asset");
            let supply = db.get_sudt_total_supply(id)?.to_string();
            assets.push((id, th, supply));
        }
    }
    ensure!(
        db.get_script_hash(db.tip.account_count)? == [0; 32],
        "Unbounded account count"
    );
    let mut rows = vec![];
    let mut balances = vec![];
    let mut requested = vec![];
    for (id, th, supply) in assets {
        let mut sum = BigUint::default();
        for chunk in registrations.chunks(256) {
            use gw_common::state::{build_account_key, build_sudt_key, SUDT_KEY_FLAG_BALANCE};
            let keys = chunk
                .iter()
                .map(|(_, reg)| build_account_key(id, &build_sudt_key(SUDT_KEY_FLAG_BALANCE, reg)))
                .collect::<Vec<_>>();
            for ((owner, reg), raw) in chunk.iter().zip(db.raw_many(&keys)?) {
                let q = BigUint::from_bytes_le(&raw);
                sum += &q;
                if q != BigUint::default() {
                    balances.push(json!({"account_id":owner,"registry_id":reg.registry_id,"registry_address":hx(&reg.address),"asset_account_id":id,"type_hash":th,"raw_amount":q.to_string()}));
                }
            }
        }
        let factor = if th == NATIVE { "10000000000" } else { "1" };
        rows.push(json!({"asset_account_id":id,"type_hash":th,"total_supply_raw":supply,"registered_balance_sum_raw":sum.to_string(),"registered_sum_matches_supply":sum.to_string()==supply,"l2_raw_per_l1_raw":factor}));
        if let Some(e) = eth {
            let reg = RegistryAddress::new(2, e.to_vec());
            requested.push(json!({"asset_account_id":id,"type_hash":th,"balance_raw":db.get_sudt_balance(id,&reg)?.to_string()}));
        }
    }
    Ok(
        json!({"schema":"l2-db-assets-1","protocol":"v1","rollup_type_hash":ROLLUP_HASH,"root":db.root(),"tip":db.tip,"read_only":true,"assets":rows,"accounts":accounts,"balances":balances,"eth_address":eth.map(hx),"address_info":eth.as_ref().map(|e|db.lookup_eth(e)).transpose()?,"address_assets":requested,"authenticated_reads":db.checked_reads.get(),"limitations":["Native CKB and L2 sUDT balances only; ERC20 protocol receipts and indirect entitlements are separate.","Registered-account sum may omit nonstandard addresses; a supply difference is reported rather than hidden.","No L1 observation, custody coverage or refund entitlement is asserted."]}),
    )
}
fn v0_db(path: &Path, eth: Option<[u8; 20]>) -> Result<Value> {
    let mut db = v0::V0Db::open(path)?;
    let leaves = db.rebuild()?;
    ensure!(
        db.root() == db.tip["account_root"],
        "DB root differs from tip"
    );
    let count = u32::try_from(db.tip["account_count"].as_u64().context("Account count")?)?;
    ensure!(count < 1_000_000, "Account count limit");
    let mut accounts = Vec::with_capacity(count as usize);
    let mut shorts = Vec::with_capacity(count as usize);
    let mut assets = vec![];
    let mut seen = BTreeSet::new();
    let mut asset_types = BTreeSet::new();
    for id in 0..count {
        let (sh, script) = NativeDb::account(&db, id)?;
        let args = bytes(&script.args)?;
        ensure!(args.starts_with(&hash32(v0::ROLLUP)?), "Foreign account");
        let short: &[u8] = &sh[..20];
        ensure!(seen.insert(short.to_vec()), "Short-address collision");
        shorts.push(short.to_vec());
        let owner = if script.code_hash == v0::ETH && script.hash_type == "type" && args.len() == 52
        {
            Some(hx(&args[32..]))
        } else {
            None
        };
        accounts.push(json!({"account_id":id,"script_hash":hx(sh),"l2_short_address":hx(short),"owner_eth_address":owner,"script":script}));
        if script.code_hash == v0::SUDT && script.hash_type == "type" {
            ensure!(args.len() == 64, "Invalid sUDT args");
            let th = hx(&args[32..]);
            ensure!(asset_types.insert(th.clone()), "Duplicate native asset");
            assets.push((id, th));
        }
    }
    ensure!(
        assets.iter().any(|(id, th)| *id == 1 && th == NATIVE),
        "Missing native CKB"
    );
    let info = eth
        .as_ref()
        .map(|e| NativeDb::lookup_eth(&db, e))
        .transpose()?;
    let requested_id = info.as_ref().and_then(|i| i["account_id"].as_u64());
    let mut rows = vec![];
    let mut balances = vec![];
    let mut requested = vec![];
    for (aid, th) in assets {
        let mut sum = BigUint::default();
        let mut user = BigUint::default();
        for (chunk_index, chunk) in shorts.chunks(512).enumerate() {
            let keys: Vec<_> = chunk.iter().map(|s| v0::balance_key(aid, s)).collect();
            let values = db.raw_many(&keys)?;
            for (off, v) in values.iter().enumerate() {
                ensure!(v[16..] == [0; 16], "Noncanonical v0 balance");
                let q = BigUint::from_bytes_le(&v[..16]);
                let id = chunk_index * 512 + off;
                sum += &q;
                if requested_id == Some(id as u64) {
                    user = q.clone();
                }
                if q != BigUint::default() {
                    balances.push(json!({"account_id":id,"asset_account_id":aid,"type_hash":th,"raw_amount":q.to_string(),"l2_short_address":hx(&shorts[id]),"owner_eth_address":accounts[id]["owner_eth_address"]}));
                }
            }
        }
        rows.push(json!({"asset_account_id":aid,"type_hash":th,"total_supply_raw":Value::Null,"registered_balance_sum_raw":sum.to_string(),"registered_sum_matches_supply":Value::Null,"l2_raw_per_l1_raw":"1"}));
        if eth.is_some() {
            requested.push(json!({"asset_account_id":aid,"type_hash":th,"balance_raw":if requested_id.is_some(){Some(user.to_string())}else{None}}));
        }
    }
    Ok(
        json!({"schema":"l2-db-assets-1","protocol":"v0","rollup_type_hash":v0::ROLLUP,"root":db.root(),"tip":db.tip,"read_only":true,"assets":rows,"accounts":accounts,"balances":balances,"eth_address":eth.map(hx),"address_info":info,"address_assets":requested,"rebuilt_leaves":leaves,"limitations":["Registered short-address native balances only; no v1-style total supply exists. Unmapped addresses are not enumerated by this query.","An ETH owner is resolved to its authenticated v0 short address, not used directly as a balance key.","No L1 observation or indirect protocol entitlement is asserted."]}),
    )
}
pub fn l1_custodian(protocol: Protocol, observation: Value) -> Result<Value> {
    let mut sums = BTreeMap::<String, (BigUint, BigUint)>::new();
    let (root, tip, rollup) = match protocol {
        Protocol::V1 => {
            let bundle: Bundle = serde_json::from_value(observation.clone())?;
            let (state, cfg) = bundle.validate()?;
            let (pool, _) = audit::inventory(&bundle, &state, &cfg)?;
            for (th, p) in pool {
                if !p.buckets.contains_key("finalized_custodian")
                    && !p.buckets.contains_key("unfinalized_custodian")
                {
                    continue;
                }
                let get = |k: &str| {
                    p.buckets
                        .get(k)
                        .map(|b| b.amount.clone())
                        .unwrap_or_default()
                };
                sums.insert(
                    th,
                    (get("finalized_custodian"), get("unfinalized_custodian")),
                );
            }
            (state.account_root.clone(), json!(state), ROLLUP_HASH)
        }
        Protocol::V0 => {
            v0::l1_report(&observation)?;
            let data = bytes(
                observation["rollup_cell"]["output_data"]
                    .as_str()
                    .context("State")?,
            )?;
            let finalized = u64::from_le_bytes(data[180..188].try_into()?);
            let cells: Vec<Cell> =
                serde_json::from_value(observation["cells"]["custodian"].clone())?;
            for c in cells {
                let args = bytes(&c.output.lock.args)?;
                let fields = v0::table(&args[32..], 3)?;
                let dep = u64::from_le_bytes(fields[2].try_into()?);
                let eligible = dep <= finalized;
                let capacity = quantity(&c.output.capacity)?;
                let cell_data = bytes(&c.output_data)?;
                ensure!(
                    capacity
                        >= c.output
                            .packed()?
                            .occupied_capacity_bytes(cell_data.len())?,
                    "Cell below occupied capacity"
                );
                let entry = sums.entry(NATIVE.into()).or_default();
                if eligible {
                    entry.0 += capacity;
                } else {
                    entry.1 += capacity;
                }
                if let Some(t) = c.output.type_script {
                    let q = BigUint::from_bytes_le(&cell_data);
                    let entry = sums.entry(t.hash()?).or_default();
                    if eligible {
                        entry.0 += q;
                    } else {
                        entry.1 += q;
                    }
                }
            }
            (
                hx(&data[32..64]),
                json!({"hash":hx(&data[140..172]),"block_count":u64::from_le_bytes(data[100..108].try_into()?),"finalized_block_number":finalized}),
                v0::ROLLUP,
            )
        }
    };
    let assets:Vec<_>=sums.into_iter().map(|(th,(f,u))|json!({"type_hash":th,"finalized_raw":f.to_string(),"unfinalized_raw":u.to_string(),"total_raw":(&f+&u).to_string()})).collect();
    Ok(
        json!({"schema":"l1-custodian-assets-1","protocol":protocol.name(),"rollup_type_hash":rollup,"account_root":root,"state":tip,"assets":assets,"observation":observation,"limitations":["Custodian only: pending deposits, existing withdrawals, stake and challenge cells are excluded.","Capture is a non-atomic single-provider observation; saved snapshots do not establish current liveness.","CKB includes occupied token-cell capacity; no refund authority is implied."]}),
    )
}
pub fn check_backing(l2: &Value, l1: &Value) -> Result<Value> {
    ensure!(
        l2["schema"] == "l2-db-assets-1" && l1["schema"] == "l1-custodian-assets-1",
        "Wrong report schema"
    );
    ensure!(
        l2["protocol"] == l1["protocol"] && l2["rollup_type_hash"] == l1["rollup_type_hash"],
        "Cross-protocol comparison"
    );
    ensure!(l2["root"] == l1["account_root"], "Account roots differ");
    let mut custody = BTreeMap::new();
    for a in l1["assets"].as_array().context("L1 assets")? {
        ensure!(
            custody
                .insert(a["type_hash"].as_str().context("type")?, a)
                .is_none(),
            "Duplicate L1 asset"
        );
    }
    let mut rows = vec![];
    let mut seen = BTreeSet::new();
    for a in l2["assets"].as_array().context("L2 assets")? {
        let th = a["type_hash"].as_str().context("type")?;
        ensure!(seen.insert(th), "Duplicate L2 asset");
        let c = custody.remove(th);
        let scale: BigUint = a["l2_raw_per_l1_raw"].as_str().context("scale")?.parse()?;
        let expected = if l2["protocol"] == "v1" && th == NATIVE {
            "10000000000"
        } else {
            "1"
        };
        ensure!(a["l2_raw_per_l1_raw"] == expected, "Invalid unit scale");
        let supply = a["total_supply_raw"].as_str();
        let sum: BigUint = a["registered_balance_sum_raw"]
            .as_str()
            .context("sum")?
            .parse()?;
        let liability: BigUint = supply
            .unwrap_or(a["registered_balance_sum_raw"].as_str().unwrap())
            .parse()?;
        let total: BigUint = c
            .and_then(|c| c["total_raw"].as_str())
            .unwrap_or("0")
            .parse()?;
        let finalized: BigUint = c
            .and_then(|c| c["finalized_raw"].as_str())
            .unwrap_or("0")
            .parse()?;
        let unfinalized: BigUint = c
            .and_then(|c| c["unfinalized_raw"].as_str())
            .unwrap_or("0")
            .parse()?;
        ensure!(&finalized + unfinalized == total, "L1 amount partition");
        rows.push(json!({"type_hash":th,"liability_raw":liability.to_string(),"liability_basis":if supply.is_some(){"total_supply"}else{"registered_sum_only"},"registered_sum_matches_supply":supply.map(|_|sum==liability),"custody_total_l2_raw":(&total*&scale).to_string(),"custody_finalized_l2_raw":(&finalized*&scale).to_string(),"delta_l2_raw":(BigInt::from(&total*&scale)-BigInt::from(liability.clone())).to_string(),"covers_selected_basis":&finalized*&scale>=liability}));
    }
    Ok(
        json!({"schema":"backing-comparison-1","protocol":l2["protocol"],"root":l2["root"],"assets":rows,"custody_without_l2_asset":custody.values().collect::<Vec<_>>(),"limitations":["V0 registered-address sums do not establish exhaustive supply coverage.","Matching account roots do not by themselves prove identical block tips or settle the historical V0 empty-successor case.","No protocol-derived refund calculation or authorization."]}),
    )
}
