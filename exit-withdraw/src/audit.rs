use crate::{db::Database, model::*};
use anyhow::{ensure, Context, Result};
use gw_common::state::State;
use gw_types::{core::Timepoint, packed, prelude::*, registry_address::RegistryAddress, U256};
use num_bigint::{BigInt, BigUint};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Default, Debug)]
pub struct Bucket {
    pub cells: u64,
    pub amount: BigUint,
    pub occupied_capacity: BigUint,
}
#[derive(Clone, Default, Debug)]
pub struct PoolAsset {
    pub type_script: Option<Script>,
    pub buckets: BTreeMap<String, Bucket>,
}
fn big(n: U256) -> BigUint {
    let mut b = [0; 32];
    n.to_little_endian(&mut b);
    BigUint::from_bytes_le(&b)
}
fn add_bucket(asset: &mut PoolAsset, bucket: &str, amount: BigUint, occupied: u64) {
    let b = asset.buckets.entry(bucket.into()).or_default();
    b.cells += 1;
    b.amount += amount;
    b.occupied_capacity += occupied;
}
pub fn finalized(tp: u64, state: &GlobalState, cfg: &Config) -> bool {
    match Timepoint::from_full_value(tp) {
        Timepoint::BlockNumber(n) => {
            n.saturating_add(cfg.finality_blocks) <= state.block_count.saturating_sub(1)
        }
        Timepoint::Timestamp(t) => match Timepoint::from_full_value(state.finalized_timepoint) {
            Timepoint::Timestamp(f) => t <= f,
            _ => false,
        },
    }
}
pub fn inventory(
    bundle: &Bundle,
    state: &GlobalState,
    cfg: &Config,
) -> Result<(BTreeMap<String, PoolAsset>, Vec<Value>)> {
    let mut assets: BTreeMap<String, PoolAsset> = BTreeMap::new();
    let mut details = Vec::new();
    for (family, cells) in &bundle.cells {
        for c in cells {
            let data = bytes(&c.output_data)?;
            let capacity = quantity(&c.output.capacity)?;
            let occupied = c.output.packed()?.occupied_capacity_bytes(data.len())?;
            ensure!(capacity >= occupied, "Cell below occupied capacity");
            let timepoint = if family == "custodian" {
                let args = bytes(&c.output.lock.args)?;
                let lock = packed::CustodianLockArgs::from_slice(
                    args.get(32..).context("Missing custodian args")?,
                )
                .context("Invalid CustodianLockArgs")?;
                Some(lock.deposit_finalized_timepoint().unpack())
            } else {
                None
            };
            let bucket = match timepoint {
                Some(t) if finalized(t, state, cfg) => "finalized_custodian",
                Some(_) => "unfinalized_custodian",
                None => family,
            };
            add_bucket(
                assets.entry(NATIVE.into()).or_default(),
                bucket,
                capacity.into(),
                occupied,
            );
            let (asset_id, amount) = if let Some(token) = &c.output.type_script {
                ensure!(
                    token.code_hash == cfg.hashes["l1_sudt"]
                        && token.hash_type == "type"
                        && bytes(&token.args)?.len() == 32
                        && data.len() == 16,
                    "Unsupported L1 token Script or data"
                );
                let id = token.hash()?;
                let amount = BigUint::from_bytes_le(&data);
                let asset = assets.entry(id.clone()).or_insert_with(|| PoolAsset {
                    type_script: Some(token.clone()),
                    ..Default::default()
                });
                ensure!(
                    asset.type_script.as_ref() == Some(token),
                    "Token Script identity conflict"
                );
                add_bucket(asset, bucket, amount.clone(), occupied);
                (id, Some(amount.to_string()))
            } else {
                ensure!(data.is_empty(), "Unexpected CKB-only cell data");
                (NATIVE.into(), None)
            };
            details.push(json!({"family":family,"bucket":bucket,"out_point":c.out_point,"asset_id":asset_id,
            "capacity_shannons":capacity.to_string(),"token_amount_raw":amount,"occupied_capacity_shannons":occupied.to_string(),"deposit_timepoint":timepoint}));
        }
    }
    Ok((assets, details))
}
struct L2Asset {
    account_id: u32,
    script_hash: String,
    type_script: Option<Script>,
    supply: BigUint,
}
fn check(checks: &mut Vec<Value>, name: &str, ok: bool, detail: &str) {
    checks.push(json!({"name":name,"status":if ok{"PASS"}else{"FAIL"},"detail":detail}));
}
fn amount(pool: &PoolAsset, bucket: &str) -> BigUint {
    pool.buckets
        .get(bucket)
        .map(|b| b.amount.clone())
        .unwrap_or_default()
}
fn signed_delta(backing: &BigUint, supply: &BigUint) -> String {
    (BigInt::from(backing.clone()) - BigInt::from(supply.clone())).to_string()
}
fn display(value: &BigUint, decimals: Option<u64>) -> Option<String> {
    let d = decimals?;
    if d > 80 {
        return None;
    }
    let scale = BigUint::from(10u8).pow(d as u32);
    if d == 0 {
        return Some(value.to_string());
    }
    Some(format!(
        "{}.{:0>width$}",
        value / &scale,
        (value % &scale).to_string(),
        width = d as usize
    ))
}
pub fn run(
    db: &Database,
    bundle: &Bundle,
    eth: Option<[u8; 20]>,
    sum_balances: bool,
) -> Result<Value> {
    let (state, cfg) = bundle.validate()?;
    let mut checks = Vec::new();
    let mut findings = Vec::new();
    check(
        &mut checks,
        "db_rollup_identity",
        hx(db.chain_id) == ROLLUP_HASH,
        "DB CHAIN_ID is the rollup type hash",
    );
    check(
        &mut checks,
        "db_tree_vs_db_block_root",
        db.root() == db.tip.account_root,
        "Stored SMT root branch vs DB last-valid block",
    );
    check(
        &mut checks,
        "db_tree_vs_l1_root",
        db.root() == state.account_root,
        "DB SMT root vs on-chain account root",
    );
    check(
        &mut checks,
        "db_block_vs_l1_tip",
        db.tip.hash == state.tip_hash && db.tip.number == state.block_count - 1,
        "Exact L2 tip hash and number",
    );
    check(
        &mut checks,
        "db_count_vs_l1_count",
        db.tip.account_count == state.account_count,
        "Account count comparison",
    );
    check(
        &mut checks,
        "rollup_running",
        state.status == 0,
        "Halting/challenge state needs separate resolution",
    );
    let comparable = checks.iter().all(|c| c["status"] == "PASS");
    let (pool, cells) = inventory(bundle, &state, &cfg)?;
    let mut assets: BTreeMap<String, L2Asset> = BTreeMap::new();
    let mut registrations = Vec::new();
    let mut seen = BTreeSet::new();
    let mut codes: BTreeMap<String, u32> = BTreeMap::new();
    eprintln!(
        "Authenticating all {} DB account Scripts...",
        db.tip.account_count
    );
    for id in 0..db.tip.account_count {
        let (sh, script) = db.account(id)?;
        *codes.entry(script.code_hash.clone()).or_default() += 1;
        let args = bytes(&script.args)?;
        ensure!(
            args.starts_with(&db.chain_id),
            "Account {id} has wrong rollup prefix"
        );
        if script.code_hash == cfg.hashes["l2_sudt"] && script.hash_type == "type" {
            ensure!(args.len() == 64, "Invalid L2 sUDT args for account {id}");
            let asset_hash: [u8; 32] = args[32..].try_into()?;
            let asset_id = hx(asset_hash);
            let mut script = if asset_id == NATIVE {
                None
            } else {
                db.script(&asset_hash, true)?
            };
            if script.is_none() {
                script = pool.get(&asset_id).and_then(|p| p.type_script.clone())
            }
            if let Some(s) = &script {
                ensure!(
                    s.hash()? == asset_id
                        && s.code_hash == cfg.hashes["l1_sudt"]
                        && s.hash_type == "type"
                        && bytes(&s.args)?.len() == 32,
                    "Unsupported DB asset Script"
                );
            }
            let a = L2Asset {
                account_id: id,
                script_hash: hx(sh),
                type_script: script,
                supply: big(db.get_sudt_total_supply(id)?),
            };
            ensure!(
                assets.insert(asset_id, a).is_none(),
                "Duplicate L2 asset identity"
            );
        }
        if sum_balances {
            if let Some(address) = db.registry(&sh)? {
                ensure!(
                    seen.insert(address.to_bytes()),
                    "Duplicate registry address"
                );
                registrations.push(address);
            }
        }
        if (id + 1) % 1000 == 0 {
            eprintln!("Authenticated {}/{} accounts", id + 1, db.tip.account_count);
        }
    }
    ensure!(
        db.get_script_hash(db.tip.account_count)? == [0; 32],
        "DB account count does not bound state"
    );
    ensure!(
        assets.get(NATIVE).map(|a| a.account_id) == Some(1),
        "Missing native CKB account 1"
    );
    let address_info = eth.as_ref().map(|a| db.lookup_eth(a)).transpose()?;
    let registry = eth.map(|a| RegistryAddress::new(2, a.to_vec()));
    let label_list: Vec<Value> =
        serde_json::from_str(include_str!("../config/mainnet-v1-token-labels.json"))?;
    let labels: BTreeMap<_, _> = label_list
        .iter()
        .filter_map(|v| {
            Some((
                v["erc20Info"]["sudtScriptArgs"].as_str()?.to_lowercase(),
                &v["info"],
            ))
        })
        .collect();
    let keys: BTreeSet<String> = assets.keys().chain(pool.keys()).cloned().collect();
    let mut rows = Vec::new();
    let mut address_rows = Vec::new();
    for asset_id in keys {
        let a = assets.get(&asset_id);
        let p = pool.get(&asset_id).cloned().unwrap_or_default();
        let native = asset_id == NATIVE;
        let script = a
            .and_then(|x| x.type_script.clone())
            .or(p.type_script.clone());
        let info = script.as_ref().and_then(|s| labels.get(&s.args));
        let symbol = if native {
            Some("CKB")
        } else {
            info.and_then(|v| v["symbol"].as_str())
        };
        let decimals = if native {
            Some(8)
        } else {
            info.and_then(|v| v["decimals"].as_u64())
        };
        let scale = BigUint::from(if native { 10_000_000_000u64 } else { 1 });
        let f = amount(&p, "finalized_custodian");
        let u = amount(&p, "unfinalized_custodian");
        let total = &f + &u;
        let total_l2 = &total * &scale;
        let f_l2 = &f * &scale;
        let status = if !comparable {
            "NOT_COMPARABLE"
        } else {
            match a {
                None => {
                    if total != BigUint::default() {
                        "UNMAPPED_CUSTODIAN"
                    } else {
                        "NO_L2_ACCOUNT"
                    }
                }
                Some(a) if total_l2 < a.supply => "DEFICIT",
                Some(a) if f_l2 < a.supply => "AWAITING_FINALITY",
                Some(a) if total_l2 > a.supply => "SURPLUS",
                _ => "MATCH",
            }
        };
        let mut summed = None;
        if sum_balances {
            if let Some(a) = a {
                eprintln!(
                    "Summing {} registered balances for asset account {}...",
                    registrations.len(),
                    a.account_id
                );
                let mut sum = BigUint::default();
                for addr in &registrations {
                    sum += big(db.get_sudt_balance(a.account_id, addr)?);
                }
                check(
                    &mut checks,
                    &format!("balance_sum_account_{}", a.account_id),
                    sum == a.supply,
                    "All registered-address balances vs total_supply",
                );
                summed = Some(sum.to_string());
            }
        }
        let buckets:BTreeMap<_,_>=p.buckets.iter().map(|(k,b)|(k,json!({"cells":b.cells,"amount":b.amount.to_string(),"occupied_capacity":b.occupied_capacity.to_string()}))).collect();
        rows.push(json!({"asset_id":asset_id,"symbol":symbol,"decimals":decimals,"type_script":script,
            "l2_account_id":a.map(|a|a.account_id),"l2_script_hash":a.map(|a|&a.script_hash),"l2_total_supply_raw":a.map(|a|a.supply.to_string()),
            "l2_registered_balance_sum_raw":summed,"l2_raw_per_l1_raw":scale.to_string(),"l2_liability_l1_floor":a.map(|a|(&a.supply/&scale).to_string()),
            "l2_remainder_raw":a.map(|a|(&a.supply%&scale).to_string()),"custodian_finalized_raw":f.to_string(),"custodian_unfinalized_raw":u.to_string(),
            "custodian_total_raw":total.to_string(),"delta_l2_raw":a.map(|a|signed_delta(&total_l2,&a.supply)),"finalized_delta_l2_raw":a.map(|a|signed_delta(&f_l2,&a.supply)),
            "custodian_display":display(&total,decimals),"status":status,"buckets":buckets}));
        if let Some(a) = a {
            if &a.supply % &scale != BigUint::default() {
                findings.push(format!(
                    "{asset_id}: sub-shannon L2 remainder requires explicit handling"
                ));
            }
            if !native && script.is_none() {
                findings.push(format!("{asset_id}: L1 Script preimage missing"));
            }
            if let Some(addr) = &registry {
                let balance = big(db.get_sudt_balance(a.account_id, addr)?);
                address_rows.push(json!({"asset_id":asset_id,"symbol":symbol,"l2_account_id":a.account_id,"balance_l2_raw":balance.to_string(),
                    "amount_l1_floor":(&balance/&scale).to_string(),"remainder_l2_raw":(&balance%&scale).to_string(),"display":display(&balance,if native{Some(18)}else{decimals}),"type_script":script}));
            }
        }
    }
    if address_info
        .as_ref()
        .is_some_and(|i| i["status"] == "unregistered")
    {
        findings.push("Requested ETH address is unregistered; balances/non-membership are authenticated separately".into());
    }
    check(
        &mut checks,
        "db_queried_state_proofs",
        true,
        &format!(
            "{} DB reads verified with native Godwoken SMT proofs; unqueried state not scanned",
            db.checked_reads.get()
        ),
    );
    let fail = checks.iter().any(|c| c["status"] == "FAIL")
        || rows.iter().any(|r| {
            matches!(
                r["status"].as_str(),
                Some("DEFICIT" | "UNMAPPED_CUSTODIAN" | "NOT_COMPARABLE")
            )
        });
    let review = !findings.is_empty()
        || rows
            .iter()
            .any(|r| matches!(r["status"].as_str(), Some("SURPLUS" | "AWAITING_FINALITY")));
    Ok(
        json!({"schema_version":1,"implementation":"rust","generated_at":now(),"network":"mainnet_v1","overall_status":if fail{"FAIL"}else if review{"REVIEW"}else{"PASS"},
        "scope":"all_l2_sudt_accounts_including_native_ckb","db_path":db.path,"db_tip":db.tip,"db_smt_root":db.root(),"db_schema_version_hex":db.schema_version,
        "l1_state":state,"l1_rollup_outpoint":bundle.rollup_cell.out_point,"l1_observed_start":bundle.observed_start,"l1_observed_end":bundle.observed_end,
        "checks":checks,"findings":findings,"assets":rows,"cells":cells,"authenticated_db_reads":db.checked_reads.get(),"account_code_hash_counts":codes,
        "registered_addresses_summed":if sum_balances{Some(registrations.len())}else{None},"eth_address":eth.map(hx),"address_info":address_info,"address_assets":address_rows,
        "limitations":["Scope is native CKB and all L2 sUDT accounts; arbitrary EVM ERC20/NFT/DeFi positions are not enumerated.",
            "Liability is authenticated total_supply; registered balance summation is performed only with --sum-balances.",
            "CKB backing includes token-cell capacity; occupied capacity and future fees require separate funding decisions.",
            "L1 inventory is a single-provider observation, not independently authenticated historical UTXO completeness.",
            "Native Godwoken SMT proofs verify queried values and absence; unqueried storage is not exhaustively rebuilt.",
            "This is a consistency/backing check, not authorization to upgrade or distribute funds."]}),
    )
}
