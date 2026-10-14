//! V1 offline final tables. All monetary fields derive from authenticated native
//! balances and freshly calculated source lots. Custody observations are separate.
use anyhow::{ensure, Context, Result};
use num_bigint::BigUint as B;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
fn ar(v: &Value) -> Result<&Vec<Value>> {
    v.as_array().context("V1 report array")
}
fn id(v: &Value) -> Result<u64> {
    v.as_u64().context("V1 report id")
}
fn s(v: &Value) -> Result<&str> {
    v.as_str().context("V1 report string")
}
fn n(v: &Value) -> Result<B> {
    Ok(s(v)?.parse()?)
}
fn display(q: &B, decimals: u64) -> String {
    if decimals == 0 {
        return q.to_string();
    }
    let t = format!("{:0>width$}", q, width = decimals as usize + 1);
    format!(
        "{}.{}",
        &t[..t.len() - decimals as usize],
        &t[t.len() - decimals as usize..]
    )
}
fn payable(q: &B, asset: u64) -> B {
    if asset == 1 {
        q / B::from(10u8).pow(10)
    } else {
        q.clone()
    }
}
fn scale(asset: u64) -> B {
    if asset == 1 {
        B::from(10u8).pow(10)
    } else {
        1u8.into()
    }
}
pub fn calculate(native: &Value, state: &Value, allocation: &Value) -> Result<Value> {
    ensure!(
        native["root"] == state["root"]
            && native["root"] == allocation["root"]
            && allocation["complete_refund_calculation"] == true,
        "Incomplete/mixed V1 calculation"
    );
    let metadata: Value = serde_json::from_slice(include_bytes!("policy/v1-assets.json"))?;
    let assets = ar(&metadata)?
        .iter()
        .map(|a| Ok((id(&a["account_id"])?, a)))
        .collect::<Result<BTreeMap<_, _>>>()?;
    ensure!(
        assets.len() == ar(&native["assets"])?.len(),
        "Native asset catalog changed"
    );
    for a in ar(&native["assets"])? {
        ensure!(
            assets
                .get(&id(&a["asset_account_id"])?)
                .context("Unreviewed native asset")?["type_hash"]
                == a["type_hash"],
            "Asset identity changed"
        );
    }
    let identities = ar(&state["accounts"])?
        .iter()
        .filter_map(|a| a["address"].as_str().map(|s| (s, a)))
        .collect::<BTreeMap<_, _>>();
    let by_id = ar(&state["accounts"])?
        .iter()
        .map(|a| Ok((id(&a["account_id"])?, a)))
        .collect::<Result<BTreeMap<_, _>>>()?;
    let authorities = ar(&allocation["authorities"])?
        .iter()
        .map(|a| Ok((id(&a["account_id"])?, a)))
        .collect::<Result<BTreeMap<_, _>>>()?;
    let mut sums = BTreeMap::<(u64, u64), B>::new();
    let mut source_sums = BTreeMap::<(u64, u64), (B, B)>::new();
    let mut paths = BTreeMap::<(u64, u64), BTreeSet<String>>::new();
    let mut claims = vec![];
    let mut retained = vec![];
    let mut held_controls = BTreeSet::new();
    for lot in ar(&allocation["source_lots"])? {
        let source = id(&lot["source_account_id"])?;
        let asset = id(&lot["asset_id"])?;
        let q = n(&lot["entitlement_raw"])?;
        let status = s(&lot["status"])?;
        ensure!(
            !status.contains("UNRESOLVED") && !status.starts_with("PENDING_"),
            "Incomplete allocation status"
        );
        let totals = source_sums.entry((source, asset)).or_default();
        totals.0 += &q;
        let address = s(&lot["beneficiary_address"])?;
        if matches!(status, "REFUND_EOA" | "REFUND_CONTROL") {
            ensure!(super::v1::SUPPORTED.contains(&asset), "Excluded asset cannot be refunded");
            ensure!(
                !matches!(
                    address,
                    "0x0000000000000000000000000000000000000000"
                        | "0x000000000000000000000000000000000000dead"
                ),
                "Locked address cannot receive refund"
            );
            let who = identities
                .get(address)
                .context("Unregistered beneficiary")?;
            let owner = id(&who["account_id"])?;
            ensure!(
                who["kind"] == "eth_eoa" || authorities.contains_key(&owner),
                "Unauthenticated beneficiary authority"
            );
            *sums.entry((owner, asset)).or_default() += &q;
            totals.1 += &q;
            let mut path = ar(&lot["path"])?
                .iter()
                .map(s)
                .collect::<Result<Vec<_>>>()?
                .join(" > ");
            if let Some(wrapper) = lot["rational_wrapper_account_id"].as_u64() {
                path.push_str(&format!(" > rational wrapper {wrapper} > {address}"));
            }
            paths
                .entry((owner, asset))
                .or_default()
                .insert(path.clone());
            claims.push(json!({"account_id":owner,"owner_eth_address":who["owner_eth_address"],"asset_id":asset,"entitlement_raw":q.to_string(),"source_account_id":source,"protocol_path":path,"disposition":status}));
        } else {
            retained.push(lot.clone());
            if let Some(who) = identities.get(address) {
                let owner = id(&who["account_id"])?;
                if [263, 4466, 4831, 5707, 5713, 10288, 10317, 12228, 12229].contains(&owner)
                    && !(owner == 10317 && asset == 1)
                {
                    held_controls.insert((owner, asset));
                }
            }
        }
    }
    let mut sources = vec![];
    for source in ar(&native["balances"])? {
        let owner = id(&source["account_id"])?;
        let asset = id(&source["asset_account_id"])?;
        let input = n(&source["raw_amount"])?;
        let (routed, refund) = source_sums.remove(&(owner, asset)).unwrap_or_default();
        ensure!(
            input == routed && refund <= input,
            "Source conservation {owner}/{asset}"
        );
        sources.push(json!({"source_account_id":owner,"asset_id":asset,"source_short_address":null,"native_input_raw":input.to_string(),"refund_entitlement_raw":refund.to_string(),"retained_raw":(&input-refund).to_string(),"conservation_pass":true}));
    }
    ensure!(
        source_sums.is_empty(),
        "Allocations reference nonexistent native sources"
    );
    let mut capacity = vec![];
    let mut caps = BTreeMap::new();
    let owners = sums.keys().map(|k| k.0).collect::<BTreeSet<_>>();
    for owner in owners {
        let own = payable(sums.get(&(owner, 1)).unwrap_or(&B::default()), 1);
        let tokens = sums
            .iter()
            .filter(|((o, a), q)| *o == owner && *a != 1 && **q != B::default())
            .count();
        let minimum = B::from(if tokens == 0 { 63 } else { 144 * tokens as u64 }) * 100000000u64;
        let topup = if own < minimum {
            &minimum - &own
        } else {
            B::default()
        };
        caps.insert(owner, (own.clone(), minimum.clone(), topup.clone()));
        capacity.push(json!({"account_id":owner,"account_kind":if authorities.contains_key(&owner){"CONTRACT"}else{"EOA"},"own_shannons":own.to_string(),"minimum_shannons_assumption":minimum.to_string(),"topup_shannons":topup.to_string(),"topup_ckb":display(&topup,8)}));
    }
    let mut accounts = vec![];
    for ((owner, asset), q) in &sums {
        let a = assets[asset];
        let decimals = id(&a["decimals"])?;
        let l1 = payable(q, *asset);
        let l2 = &l1 * scale(*asset);
        ensure!(l1.bits() <= 128, "sUDT width");
        let cap = &caps[owner];
        let ready = cap.2 == B::default();
        let control = authorities.contains_key(owner);
        let who = by_id[owner];
        let authority = if let Some(auth) = authorities.get(owner) {
            if let (Some(t), Some(m)) =
                (auth["refund_threshold"].as_u64(), auth["members"].as_u64())
            {
                format!("{t}-of-{m} distinct authenticated owners")
            } else {
                format!(
                    "Authenticated {}",
                    auth["wallet_kind"].as_str().unwrap_or("fixed controller")
                )
            }
        } else {
            "ETH owner".into()
        };
        accounts.push(json!({"account_id":owner,"account_kind":if control{"CONTRACT"}else{"EOA"},"owner_eth_address":who["owner_eth_address"],"refund_authority":authority,"asset_id":asset,"symbol":a["symbol"],"type_hash":a["type_hash"],"l2_decimals":decimals,"entitlement_raw":q.to_string(),"entitlement":display(q,decimals),"payable_l2_raw":l2.to_string(),"payable":display(&l2,decimals),"payable_l1_raw":l1.to_string(),"capacity_ready_l2_raw":if ready{l2.to_string()}else{"0".into()},"capacity_ready_l1_raw":if ready{l1.to_string()}else{"0".into()},"capacity_ready":display(&if ready{l2.clone()}else{B::default()},decimals),"rounding_retained":display(&(q-&l2),decimals),"status":match(ready,control){(true,false)=>"CAPACITY_READY",(false,false)=>"NEEDS_CKB_TOPUP",(true,true)=>"CAPACITY_READY_IMPLEMENTATION_PENDING",(false,true)=>"NEEDS_CKB_TOPUP_IMPLEMENTATION_PENDING"},"account_topup_shannons_do_not_sum_per_asset":cap.2.to_string(),"protocols_or_reason":paths[&(*owner,*asset)].iter().cloned().collect::<Vec<_>>().join("; ")}));
    }
    // Approved register marker, not a monetary entitlement: KOI is excluded
    // before protocol look-through. Preserve the control-263 exclusion row;
    // actual KOI remains fully retained in its physical source ledger.
    if authorities.contains_key(&263) {
        held_controls.insert((263, 1570));
    }
    for (owner, asset) in held_controls {
        if sums.contains_key(&(owner, asset)) {
            continue;
        }
        let a = assets[&asset];
        let d = id(&a["decimals"])?;
        accounts.push(json!({"account_id":owner,"account_kind":"CONTRACT","owner_eth_address":null,"refund_authority":"Retained under approved disposition; no current refund authority","asset_id":asset,"symbol":a["symbol"],"type_hash":a["type_hash"],"l2_decimals":d,"entitlement_raw":"0","entitlement":display(&B::default(),d),"payable_l2_raw":"0","payable_l1_raw":"0","payable":display(&B::default(),d),"capacity_ready_l2_raw":"0","capacity_ready_l1_raw":"0","capacity_ready":"0","rounding_retained":"0","status":"HELD_APPROVED_DISPOSITION","account_topup_shannons_do_not_sum_per_asset":null,"protocols_or_reason":"No refund; see retained source lots for the approved reason and exact backing."}));
    }
    accounts.sort_by_key(|a| (a["account_id"].as_u64(), a["asset_id"].as_u64()));
    // History discovers extra native owners absent from the registry census.
    // Keep them separately visible; never silently turn them into ETH claims.
    let unregistered = state["native_protocols"]["history"]["unknown_native_holders"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let mut unregistered_sums = BTreeMap::<u64, B>::new();
    let mut seen_unregistered = BTreeSet::new();
    for row in &unregistered {
        let asset = id(&row["asset_account_id"])?;
        let address = s(&row["address"])?;
        ensure!(
            !identities.contains_key(address) && seen_unregistered.insert((asset, address)),
            "Duplicate or registered supplemental native owner"
        );
        ensure!(
            assets.contains_key(&asset),
            "Unknown supplemental native asset"
        );
        *unregistered_sums.entry(asset).or_default() += n(&row["raw_amount"])?;
    }
    let mut asset_rows = vec![];
    for (asset, a) in assets {
        let fresh = ar(&native["assets"])?
            .iter()
            .find(|x| x["asset_account_id"] == asset)
            .unwrap();
        let input = sources
            .iter()
            .filter(|r| r["asset_id"] == asset)
            .map(|r| n(&r["native_input_raw"]))
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .sum::<B>();
        let refund = sums
            .iter()
            .filter(|((_, id), _)| *id == asset)
            .map(|(_, q)| q)
            .sum::<B>();
        let paid = accounts
            .iter()
            .filter(|r| r["asset_id"] == asset)
            .map(|r| n(&r["payable_l2_raw"]))
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .sum::<B>();
        let ready = accounts
            .iter()
            .filter(|r| r["asset_id"] == asset)
            .map(|r| n(&r["capacity_ready_l2_raw"]))
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .sum::<B>();
        let supply = n(&fresh["total_supply_raw"])?;
        ensure!(
            refund <= input && paid <= refund && paid <= supply,
            "V1 native backing ceiling"
        );
        asset_rows.push(json!({"asset_id":asset,"symbol":a["symbol"],"type_hash":a["type_hash"],"decimals":a["decimals"],"l2_recorded_before_raw":supply.to_string(),"discovered_l2_before_raw":input.to_string(),"refund_entitlement_raw":refund.to_string(),"refund_payable_l2_raw":paid.to_string(),"refund_payable_l1_raw":payable(&paid,asset).to_string(),"capacity_ready_l2_raw":ready.to_string(),"discovered_l2_retained_raw":(&input-&refund).to_string(),"discovered_l2_after_payable_raw":(&input-&paid).to_string(),"l2_recorded_after_payable_raw":(&supply-&paid).to_string(),"rounding_retained_raw":(&refund-&paid).to_string(),"conservation_pass":true,"supply_matches_discovered_balances":supply==input,"history_discovered_unregistered_retained_raw":unregistered_sums.get(&asset).cloned().unwrap_or_default().to_string()}));
    }
    let mut hash = Sha256::new();
    for bytes in [
        include_bytes!("v1.rs").as_slice(),
        include_bytes!("v1_disposition.rs").as_slice(),
        include_bytes!("v1_reports.rs").as_slice(),
        include_bytes!("v1_wrappers.rs").as_slice(),
        include_bytes!("v1_closeout.rs").as_slice(),
        include_bytes!("policy/v1-assets.json").as_slice(),
    ] {
        hash.update(bytes);
    }
    Ok(
        json!({"schema":"db-refund-tables-1","protocol":"v1","root":native["root"],"policy_sha256":hex::encode(hash.finalize()),"accounts":accounts,"capacity":capacity,"claim_sources":claims,"source_reserves":sources,"retained_lots":retained,"unregistered_native_reserves":unregistered,"assets":asset_rows,"authorities":allocation["authorities"],"custodian_observation":null,"complete_refund_calculation":true,"execution_ready":false,"capacity_assumption":"63 CKB plain or 144 CKB per nonzero sUDT output; excludes fees; control-wallet destination integration remains pending"}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (Value, Value, Value) {
        let metadata: Value =
            serde_json::from_slice(include_bytes!("policy/v1-assets.json")).unwrap();
        let q = (B::from(63u64) * B::from(10u8).pow(18) - 1u8).to_string();
        let address = "0x0000000000000000000000000000000000000001";
        let assets=metadata.as_array().unwrap().iter().map(|a|json!({"asset_account_id":a["account_id"],"type_hash":a["type_hash"],"total_supply_raw":if a["account_id"]==1{q.as_str()}else{"0"}})).collect::<Vec<_>>();
        (
            json!({"root":"test-root","assets":assets,"balances":[{"account_id":1,"asset_account_id":1,"raw_amount":q}]}),
            json!({"root":"test-root","accounts":[{"account_id":1,"address":address,"owner_eth_address":address,"kind":"eth_eoa"}]}),
            json!({"root":"test-root","complete_refund_calculation":true,"authorities":[],"source_lots":[{"source_account_id":1,"asset_id":1,"entitlement_raw":q,"beneficiary_address":address,"status":"REFUND_EOA","path":["direct"]}]}),
        )
    }
    #[test]
    fn capacity_is_after_whole_account_l1_rounding() {
        let (n, s, a) = fixture();
        let r = calculate(&n, &s, &a).unwrap();
        assert_eq!(r["capacity"][0]["topup_shannons"], "1");
        assert_eq!(r["accounts"][0]["payable_l1_raw"], "6299999999");
        assert_eq!(r["accounts"][0]["capacity_ready_l1_raw"], "0");
        assert_eq!(r["assets"][0]["rounding_retained_raw"], "9999999999");
    }
    #[test]
    fn source_duplication_unknown_rights_and_locked_addresses_fail() {
        let (n, s, a) = fixture();
        let mut duplicate = a.clone();
        duplicate["source_lots"]
            .as_array_mut()
            .unwrap()
            .push(a["source_lots"][0].clone());
        assert!(calculate(&n, &s, &duplicate).is_err());
        let mut unknown = a.clone();
        unknown["source_lots"][0]["status"] = json!("UNRESOLVED");
        assert!(calculate(&n, &s, &unknown).is_err());
        let mut locked = a.clone();
        locked["source_lots"][0]["beneficiary_address"] =
            json!("0x0000000000000000000000000000000000000000");
        assert!(calculate(&n, &s, &locked).is_err());
    }
    #[test]
    fn yok_is_retained_without_changing_other_entitlements_or_adding_capacity() {
        let (mut native, state, mut allocation) = fixture();
        let original = calculate(&native, &state, &allocation).unwrap();
        let amount = "100000000000000000000";
        native["assets"].as_array_mut().unwrap().iter_mut()
            .find(|a| a["asset_account_id"] == 128).unwrap()["total_supply_raw"] = json!(amount);
        native["balances"].as_array_mut().unwrap().push(json!({
            "account_id":1,"asset_account_id":128,"raw_amount":amount
        }));
        let mut lot = allocation["source_lots"][0].clone();
        lot["asset_id"] = json!(128);
        lot["entitlement_raw"] = json!(amount);
        // Even an upstream allocation claiming YOK is payable must fail closed.
        allocation["source_lots"].as_array_mut().unwrap().push(lot);
        assert!(calculate(&native, &state, &allocation).is_err());
        allocation["source_lots"][1]["status"] = json!("EXCLUDED_ASSET_SCOPE");
        let result = calculate(&native, &state, &allocation).unwrap();
        assert_eq!(result["accounts"], original["accounts"]);
        assert_eq!(result["capacity"], original["capacity"]);
        assert_eq!(result["claim_sources"], original["claim_sources"]);
        let yok = result["assets"].as_array().unwrap().iter()
            .find(|a| a["asset_id"] == 128).unwrap();
        assert_eq!(yok["refund_entitlement_raw"], "0");
        assert_eq!(yok["refund_payable_l1_raw"], "0");
        assert_eq!(yok["discovered_l2_retained_raw"], amount);
        assert_eq!(result["retained_lots"][0]["entitlement_raw"], amount);
    }
    #[test]
    fn metadata_contains_no_amounts() {
        let m: Value = serde_json::from_slice(include_bytes!("policy/v1-assets.json")).unwrap();
        assert_eq!(m.as_array().unwrap().len(), 48, "Include zero-balance native definitions");
        for a in m.as_array().unwrap() {
            assert_eq!(
                a.as_object()
                    .unwrap()
                    .keys()
                    .map(String::as_str)
                    .collect::<Vec<_>>(),
                vec!["account_id", "decimals", "symbol", "type_hash"]
            );
        }
    }
}
