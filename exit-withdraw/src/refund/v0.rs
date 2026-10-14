//! V0 native, WCKB, Yokai LP/farm, stable-pool and IFO principal reconstruction.
//! All numeric inputs come from the current invocation's authenticated DB facts.
use super::state::word;
use crate::model::{bytes, hx};
use anyhow::{ensure, Context, Result};
use num_bigint::BigUint as B;
use serde_json::{json, Value};
use sha2::Sha256;
use sha3::{Digest, Keccak256};
use std::collections::{BTreeMap, BTreeSet};
const CONTROLS: [u64; 5] = [16045, 36620, 36630, 38316, 48851];
const IFOS: [u64; 7] = [19202, 29696, 31272, 36023, 36024, 47794, 47796];
fn array(v: &Value) -> Result<&Vec<Value>> {
    v.as_array().context("array")
}
fn id(v: &Value) -> Result<u64> {
    v.as_u64().context("id")
}
fn text(v: &Value) -> Result<&str> {
    v.as_str().context("string")
}
fn dec(v: &Value) -> Result<B> {
    let s = text(v)?;
    ensure!(
        !s.is_empty() && s.bytes().all(|x| x.is_ascii_digit()),
        "canonical decimal"
    );
    Ok(s.parse()?)
}
fn hexnum(v: &Value) -> Result<B> {
    Ok(B::from_bytes_be(&bytes(text(v)?)?))
}
fn share(b: &B, w: &B, s: &B) -> Result<B> {
    ensure!(*s > B::default() && w <= s, "Invalid share");
    Ok(b * w / s)
}
fn row(v: &Value, i: u64) -> Result<&Value> {
    array(&v["rows"])?
        .iter()
        .find(|r| r["account_id"] == i)
        .with_context(|| format!("Missing state for {i}"))
}
fn key(r: &Value, k: &str) -> Result<B> {
    hexnum(
        &array(&r["slots"])?
            .iter()
            .find(|v| v["key"] == k)
            .with_context(|| format!("Missing slot {k}"))?["value"],
    )
}
fn slot(r: &Value, i: u64) -> Result<B> {
    key(r, &hx(word(i)))
}
fn mapping(r: &Value, s: u64, vyper: bool, width: u64) -> Result<&Vec<Value>> {
    let matches = array(&r["mappings"])?
        .iter()
        .filter(|m| {
            m["layout"]["slot"] == s
                && m["layout"]["slot_first"] == vyper
                && m["layout"]["width"] == width
                && m["layout"]["key_before"].is_null()
                && m["layout"]["key_after"].is_null()
        })
        .collect::<Vec<_>>();
    ensure!(matches.len() == 1, "Ambiguous or missing reviewed mapping");
    array(&matches[0]["nonzero_records"])
}
struct Ledger<'a> {
    accounts: BTreeMap<u64, &'a Value>,
    by_address: BTreeMap<String, u64>,
    eligible: BTreeSet<u64>,
    sources: BTreeMap<(u64, u64), (B, B)>,
    claims: Vec<Value>,
    used: BTreeSet<(u64, u64, u64, String)>,
}
impl Ledger<'_> {
    fn payable(&self, i: u64) -> bool {
        self.accounts
            .get(&i)
            .is_some_and(|r| r["kind"] == "eth_eoa" || CONTROLS.contains(&i))
    }
    fn amount(&self, s: u64, a: u64) -> B {
        self.sources
            .get(&(s, a))
            .map(|(n, _)| n.clone())
            .unwrap_or_default()
    }
    fn add(&mut self, owner: u64, source: u64, asset: u64, q: B, path: String) -> Result<()> {
        if q == B::default() {
            return Ok(());
        }
        ensure!(self.eligible.contains(&asset), "Excluded asset allocated");
        ensure!(self.payable(owner), "Unresolved beneficiary {owner}");
        ensure!(
            self.used.insert((owner, source, asset, path.clone())),
            "Duplicate claim path"
        );
        let (total, allocated) = self
            .sources
            .get_mut(&(source, asset))
            .context("Missing native source")?;
        ensure!(
            &*allocated + &q <= *total,
            "Source overdraw {source}/{asset}"
        );
        *allocated += &q;
        self.claims.push(json!({"account_id":owner,"source_account_id":source,"asset_id":asset,"entitlement_raw":q.to_string(),"owner_eth_address":self.accounts[&owner]["owner_eth_address"],"protocol_path":path,"multisig_approval_pending":CONTROLS.contains(&owner)}));
        Ok(())
    }
}
/// Monetary reconstruction for regression, not a release-ready refund table.
/// Multisig authority, complete disposition explanations and output capacity are separate gates.
pub fn reconstruct(native: &Value, state: &Value) -> Result<Value> {
    ensure!(
        native["protocol"] == "v0" && state["protocol"] == "v0" && native["root"] == state["root"],
        "Mixed DB facts"
    );
    ensure!(
        native["root"] == "0xbf094fbcee2258770df7749bed2ed3e2565c17c98b1f40f2ca6377d253a11d7d",
        "Unsupported V0 refund root"
    );
    let assets: Value = serde_json::from_slice(include_bytes!("policy/v0-assets.json"))?;
    let mut eligible = BTreeSet::new();
    for a in array(&assets)? {
        let aid = id(&a["account_id"])?;
        let fresh = array(&native["assets"])?
            .iter()
            .find(|x| x["asset_account_id"] == aid)
            .context("Asset missing from DB")?;
        ensure!(
            fresh["type_hash"] == a["type_hash"],
            "Native asset identity"
        );
        if a["refund_scope"] == true {
            eligible.insert(aid);
        }
    }
    let accounts = array(&state["accounts"])?
        .iter()
        .map(|a| Ok((id(&a["account_id"])?, a)))
        .collect::<Result<BTreeMap<_, _>>>()?;
    let by_address = accounts
        .iter()
        .map(|(i, a)| Ok((text(&a["l2_short_address"])?.to_owned(), *i)))
        .collect::<Result<BTreeMap<_, _>>>()?;
    let mut l = Ledger {
        accounts,
        by_address,
        eligible,
        sources: BTreeMap::new(),
        claims: vec![],
        used: BTreeSet::new(),
    };
    for b in array(&native["balances"])? {
        ensure!(
            l.sources
                .insert(
                    (id(&b["account_id"])?, id(&b["asset_account_id"])?),
                    (dec(&b["raw_amount"])?, B::default())
                )
                .is_none(),
            "Duplicate native source"
        );
    }
    let direct = l
        .sources
        .iter()
        .filter(|((s, a), _)| l.payable(*s) && l.eligible.contains(a))
        .map(|(&(s, a), (n, _))| (s, a, n.clone()))
        .collect::<Vec<_>>();
    for (s, a, n) in direct {
        l.add(
            s,
            s,
            a,
            n,
            if CONTROLS.contains(&s) {
                "direct native balance"
            } else {
                "direct_native"
            }
            .into(),
        )?;
    }
    let mut proxy = BTreeMap::new();
    for r in array(&state["rows"])? {
        if r["runtime_hash"] == "0xa816b946a890cd593f780e8b6859a9b82314c5df4c8270d66f7c502e818345dc"
        {
            let aid: u64 = slot(r, 2)?.to_string().parse()?;
            ensure!(
                array(&assets)?.iter().any(|a| a["account_id"] == aid),
                "Unknown native proxy asset"
            );
            proxy.insert(text(&r["address"])?.to_owned(), aid);
        }
    }
    let facts = &state["native_protocols"];
    let authorities = array(&facts["authorities"])?;
    ensure!(
        authorities.len() == CONTROLS.len()
            && CONTROLS.iter().all(|id| authorities
                .iter()
                .filter(|a| a["account_id"] == *id
                    && a["members"] == 3
                    && a["refund_threshold"] == 2)
                .count()
                == 1),
        "Control authority coverage"
    );
    let pairs = array(&facts["pairs"]["pairs"])?;
    let mut wrapped = BTreeMap::new();
    let mut wrapper_ids = BTreeMap::new();
    for w in array(&facts["wckb"]["wrappers"])? {
        let wid = id(&w["account_id"])?;
        ensure!(
            l.amount(wid, 1) == dec(&w["native_ckb_shannons"])?,
            "Wrapper backing mismatch"
        );
        ensure!(
            dec(&w["known_holder_sum_shannons"])? <= l.amount(wid, 1),
            "Wrapper overhang"
        );
        wrapper_ids.insert(text(&l.accounts[&wid]["l2_short_address"])?.to_owned(), wid);
        for h in array(&w["holders"])? {
            let owner = id(&h["account_id"])?;
            let amount = dec(&h["raw_wckb"])?;
            ensure!(
                wrapped.insert((wid, owner), amount.clone()).is_none(),
                "Duplicate wrapper holder"
            );
            if l.payable(owner) {
                l.add(owner, wid, 1, amount, format!("wckb:{wid}"))?;
            }
        }
    }
    let mut farms = BTreeMap::<(u64, String), Vec<(u64, B)>>::new();
    for f in array(&facts["farms"]["farms"])? {
        let fid = id(&f["account_id"])?;
        for p in array(&f["pools"])? {
            let users = farms
                .entry((fid, text(&p["lp_token"])?.into()))
                .or_default();
            for u in array(&p["users"])? {
                users.push((id(&u["account_id"])?, dec(&u["principal_raw"])?));
            }
        }
    }
    let mut sides = BTreeMap::<(u64, u64), (u64, u64, B, B)>::new();
    for p in pairs {
        let pid = id(&p["account_id"])?;
        let supply = dec(&p["effective_supply"])?;
        ensure!(
            dec(&p["total_supply"])? + dec(&p["fee_lp_to_mint"])? == supply,
            "Effective LP supply"
        );
        ensure!(
            p["fee_calculation_verified"] == true,
            "Unverified fee state {pid}"
        );
        for side in 0..2 {
            let token = text(&p[format!("token{side}")])?;
            let source = if let Some(&aid) = proxy.get(token) {
                Some((pid, aid, l.amount(pid, aid)))
            } else if let Some(&wid) = wrapper_ids.get(token) {
                Some((
                    wid,
                    1,
                    wrapped.get(&(wid, pid)).cloned().unwrap_or_default(),
                ))
            } else {
                None
            };
            let Some((source, asset, backing)) = source else {
                continue;
            };
            if !l.eligible.contains(&asset) {
                continue;
            }
            sides.insert(
                (pid, side),
                (source, asset, backing.clone(), supply.clone()),
            );
            for h in array(&p["holders"])? {
                let Some(owner) = h["account_id"].as_u64() else {
                    continue;
                };
                let weight = dec(&h["raw_lp"])?;
                if l.payable(owner) {
                    let path = if CONTROLS.contains(&owner) {
                        format!("existing LP pair {pid} side {side}")
                    } else {
                        format!("pair:{pid}:side:{side}")
                    };
                    l.add(
                        owner,
                        source,
                        asset,
                        share(&backing, &weight, &supply)?,
                        path,
                    )?;
                } else if let Some(users) = farms.get(&(owner, text(&p["address"])?.into())) {
                    let total: B = users.iter().map(|(_, n)| n.clone()).sum();
                    ensure!(
                        total == weight,
                        "Farm principal does not match held LP {owner}/{pid}"
                    );
                    for (uid, w) in users {
                        if l.payable(*uid) {
                            l.add(
                                *uid,
                                source,
                                asset,
                                share(&backing, w, &supply)?,
                                format!("farm:{owner}:pair:{pid}:side:{side}"),
                            )?;
                        }
                    }
                }
            }
            let fee = dec(&p["fee_lp_to_mint"])?;
            if fee > B::default() {
                let to = text(&p["fee_to"])?;
                ensure!(
                    l.by_address.get(to) == Some(&16045),
                    "Unapproved fee recipient {to}"
                );
                l.add(
                    16045,
                    source,
                    asset,
                    share(&backing, &fee, &supply)?,
                    format!("approved pending protocol fee for pair {pid}"),
                )?;
            }
        }
    }
    // Stable LP claims use the pool's recorded balances; excess/admin reserves remain held.
    for (pool, lp, staking) in [
        (38602, 38601, None),
        (38606, 38605, None),
        (48394, 48393, Some(48397)),
        (48666, 48665, Some(48667)),
        (48669, 48668, Some(48670)),
        (49392, 49391, Some(49393)),
    ] {
        let p = row(state, pool)?;
        let r = row(state, lp)?;
        let supply = slot(r, if lp < 40000 { 9 } else { 4 })?;
        ensure!(
            slot(p, if pool < 40000 { 5 } else { 6 })?
                == B::from_bytes_be(&bytes(text(&l.accounts[&lp]["l2_short_address"])?)?),
            "Stable LP pointer"
        );
        let holders = mapping(r, if lp < 40000 { 7 } else { 2 }, true, 1)?;
        let sum = holders
            .iter()
            .map(|u| hexnum(&u["fields"][0]))
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .sum::<B>();
        ensure!(sum == supply, "Stable supply gap");
        let mut owners = vec![];
        for u in holders {
            let owner = *l
                .by_address
                .get(text(&u["address"])?)
                .context("Unregistered stable owner")?;
            let w = hexnum(&u["fields"][0])?;
            if l.payable(owner) {
                owners.push((owner, w, format!("stable:{pool}:lp:{lp}")));
            } else {
                ensure!(Some(owner) == staking, "Unresolved stable holder");
                let st = row(state, owner)?;
                ensure!(
                    slot(st, 11)? == w
                        && slot(st, 5)? == B::from_bytes_be(&bytes(text(&r["address"])?)?),
                    "Stable staking backing"
                );
                let us = mapping(st, 12, false, 1)?;
                let total = us
                    .iter()
                    .map(|u| hexnum(&u["fields"][0]))
                    .collect::<Result<Vec<_>>>()?
                    .into_iter()
                    .sum::<B>();
                ensure!(total == w, "Stable staking census gap");
                for u in us {
                    let uid = *l
                        .by_address
                        .get(text(&u["address"])?)
                        .context("Unregistered staker")?;
                    ensure!(l.payable(uid), "Unresolved staker");
                    owners.push((
                        uid,
                        hexnum(&u["fields"][0])?,
                        format!("stake:{owner}:stable:{pool}:lp:{lp}"),
                    ));
                }
            }
        }
        for side in 0..if pool == 38602 { 3 } else { 2 } {
            let dynkey = |s| {
                let n = B::from_bytes_be(&Keccak256::digest(word(s))) + B::from(side as u64);
                format!("0x{n:0>64x}")
            };
            let token = key(p, &dynkey(0))?;
            let token = format!("0x{token:0>40x}");
            let asset = *proxy.get(&token).context("Stable native proxy")?;
            let budget = key(p, &dynkey(1))?;
            ensure!(
                budget <= l.amount(pool, asset),
                "Stable pool budget exceeds native backing"
            );
            ensure!(l.eligible.contains(&asset), "Stable excluded side");
            for (owner, w, path) in &owners {
                l.add(
                    *owner,
                    pool,
                    asset,
                    share(&budget, w, &supply)?,
                    format!("{path}:side:{side}"),
                )?;
            }
        }
    }
    let mut ifo_reports = vec![];
    for i in IFOS.into_iter().chain([49401]) {
        let r = row(state, i)?;
        let total = slot(r, 8)?;
        let raising = slot(r, 5)?;
        let users = mapping(r, 7, false, 2)?;
        let sum = users
            .iter()
            .map(|u| hexnum(&u["fields"][0]))
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .sum::<B>();
        ensure!(sum == total, "IFO participant census gap");
        let mut rights = vec![];
        for u in users {
            let d = hexnum(&u["fields"][0])?;
            let claimed = hexnum(&u["fields"][1])?;
            ensure!(claimed <= 1u8.into(), "Invalid IFO claimed flag");
            if claimed != B::default() {
                continue;
            }
            ensure!(
                total > B::default() && raising > B::default(),
                "Invalid IFO totals"
            );
            let nominal = if total > raising {
                let scale = B::from(10u8).pow(12);
                &d - (&d * &scale / &total) * &raising / scale
            } else {
                B::default()
            };
            rights.push((
                *l.by_address
                    .get(text(&u["address"])?)
                    .context("Unregistered IFO owner")?,
                nominal,
            ));
        }
        let nominal: B = rights.iter().map(|(_, n)| n.clone()).sum();
        let asset = match i {
            19202 => Some(1),
            29696 => Some(19),
            31272 => Some(30634),
            _ => None,
        };
        let pid = if i == 49401 { 48016 } else { 24035 };
        let cash = if let Some(a) = asset {
            l.amount(i, a)
        } else {
            let p = pairs
                .iter()
                .find(|p| p["account_id"] == pid)
                .context("IFO pair")?;
            array(&p["holders"])?
                .iter()
                .find(|h| h["account_id"] == i)
                .map(|h| dec(&h["raw_lp"]))
                .transpose()?
                .unwrap_or_default()
        };
        if nominal > cash {
            ensure!(IFOS.contains(&i), "Unapproved IFO shortfall");
        }
        let mut paid = B::default();
        for (owner, n) in rights {
            let q = if nominal > cash {
                share(&cash, &n, &nominal)?
            } else {
                n
            };
            paid += &q;
            if let Some(a) = asset {
                l.add(owner, i, a, q, format!("ifo:{i}:unclaimed_refund"))?;
            } else {
                for side in 0..2 {
                    if let Some((source, a, backing, supply)) = sides.get(&(pid, side)) {
                        l.add(
                            owner,
                            *source,
                            *a,
                            share(backing, &q, supply)?,
                            format!("ifo:{i}:pair:{pid}:side:{side}"),
                        )?;
                    }
                }
            }
        }
        ensure!(paid <= cash, "IFO overdraw");
        ifo_reports.push(json!({"account_id":i,"nominal_raw":nominal.to_string(),"cash_raw":cash.to_string(),"paid_raw":paid.to_string(),"pro_rata":nominal>cash}));
    }
    let mut totals = BTreeMap::<(u64, u64), B>::new();
    for c in &l.claims {
        *totals
            .entry((id(&c["account_id"])?, id(&c["asset_id"])?))
            .or_default() += dec(&c["entitlement_raw"])?;
    }
    let mut capacity = BTreeMap::<u64, (B, usize)>::new();
    for ((owner, asset), amount) in &totals {
        let entry = capacity.entry(*owner).or_default();
        if *asset == 1 {
            entry.0 = amount.clone();
        } else if *amount > B::default() {
            entry.1 += 1;
        }
    }
    let capacities=capacity.into_iter().map(|(owner,(own,tokens))| {
        let (minimum,gap)=capacity_requirement(&own,tokens);
        json!({"account_id":owner,"own_shannons":own.to_string(),"minimum_shannons_assumption":minimum.to_string(),"topup_shannons":gap.to_string(),"nonzero_sudt_count":tokens})
    }).collect::<Vec<_>>();
    let accounts:Vec<_>=totals.into_iter().map(|((owner,asset),n)|json!({"account_id":owner,"asset_id":asset,"entitlement_raw":n.to_string()})).collect();
    let mut reserves = Vec::with_capacity(l.sources.len());
    for ((source, asset), (n, used)) in l.sources {
        let account = l.accounts[&source];
        let retained = &n - &used;
        let reason = if retained == B::default() {
            "Fully allocated to verified current rights."
        } else if asset == 49721 {
            "Whole YOK token excluded in V0, including direct and indirect rights."
        } else if asset == 49787 {
            "KOI token deferred; no redistribution of retained backing."
        } else if !l.eligible.contains(&asset) {
            "Asset has no approved custodian-backed refund route."
        } else if source == 0 {
            "System account balance retained; no verified ETH beneficiary."
        } else if source == 36838 {
            "Project treasury retained: manager authority does not establish beneficial ownership; no ordinary-holder redemption right."
        } else if [12119, 12134, 28983, 37790, 37800, 49358].contains(&source) {
            "Approved low-total-value contract residual hold; preserve provenance, do not pay the administrator or original sender."
        } else if [28982, 36278, 37789].contains(&source) {
            "NFT scope deferred; associated monetary residual retained."
        } else if [38602, 38606, 48394, 48666, 48669, 49392].contains(&source) {
            "Stable-pool admin reserve, unallocated rights and integer rounding retained within this source."
        } else if IFOS.contains(&source) || source == 49401 {
            "IFO refund allocations completed under approved local-cash policy; non-refund balance and rounding retained."
        } else if account["runtime_hash"]
            == "0xf601f6484fc84f0515c0ed834594c582a3974f00e4dc030ec489e8b57c75f8da"
        {
            "LP source remainder: locked liquidity, held or unsupported rights and integer rounding; never renormalize known holders."
        } else if account["runtime_hash"]
            == "0xeff32b72882b38bb97bde14c7ac09f4cab2ef14f6e089d778a05496f554a2973"
        {
            "Wrapped CKB source remainder: unregistered/held direct or nested receipt rights and rounding."
        } else {
            anyhow::bail!("Unreviewed retained funded source {source}/{asset}");
        };
        reserves.push(json!({"source_account_id":source,"source_short_address":account["l2_short_address"],"asset_id":asset,"native_input_raw":n.to_string(),"refund_entitlement_raw":used.to_string(),"retained_raw":retained.to_string(),"conservation_pass":true,"disposition":reason}));
    }
    let mut unmapped_keys = BTreeSet::new();
    for source in array(&facts["history"]["unknown_native_holders"])? {
        let address = text(&source["short_address"])?;
        let asset = id(&source["asset_account_id"])?;
        ensure!(
            !l.by_address.contains_key(address),
            "Unmapped source has registered owner"
        );
        ensure!(
            unmapped_keys.insert((address, asset)),
            "Duplicate unmapped source"
        );
        let amount = dec(&source["raw_amount"])?;
        reserves.push(json!({"source_account_id":format!("unregistered:{address}"),"source_short_address":address,"asset_id":asset,"native_input_raw":amount.to_string(),"refund_entitlement_raw":"0","retained_raw":amount.to_string(),"conservation_pass":true,"disposition":"Unregistered address; no authenticated ETH beneficiary. Retain in source."}));
    }
    Ok(
        json!({"schema":"v0-db-allocation-candidate-1","protocol":"v0","root":native["root"],"complete_refund_calculation":false,"accounts":accounts,"claims":l.claims,"sources":reserves,"capacity":capacities,"authorities":authorities,"asset_metadata":assets,"identities":l.accounts.values().copied().collect::<Vec<_>>(),"policy_sha256":hex::encode(Sha256::digest(include_bytes!("v0.rs"))),"capacity_assumption":"63 CKB plain or 144 CKB per nonzero sUDT output; excludes network fees; multisig destination integration remains pending","ifos":ifo_reports,"remaining_gates":["Serialize approved dispositions and complete report fields.","Compare every final report field to the regression baseline."]}),
    )
}

fn capacity_requirement(own: &B, tokens: usize) -> (B, B) {
    let minimum = if tokens == 0 {
        B::from(63u8)
    } else {
        B::from(tokens) * 144u16
    } * 100_000_000u64;
    let gap = if own < &minimum {
        &minimum - own
    } else {
        B::default()
    };
    (minimum, gap)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn source_budget_and_duplicate_paths_are_enforced() {
        let account = json!({"kind":"eth_eoa","owner_eth_address":"0x0000000000000000000000000000000000000001"});
        let mut l = Ledger {
            accounts: BTreeMap::from([(2, &account)]),
            by_address: BTreeMap::new(),
            eligible: BTreeSet::from([1]),
            sources: BTreeMap::from([((9, 1), (100u8.into(), B::default()))]),
            claims: vec![],
            used: BTreeSet::new(),
        };
        l.add(2, 9, 1, 60u8.into(), "first".into()).unwrap();
        assert!(l.add(2, 9, 1, 1u8.into(), "first".into()).is_err());
        assert!(l.add(2, 9, 1, 41u8.into(), "overdraw".into()).is_err());
        assert!(l.add(2, 9, 2, 1u8.into(), "excluded".into()).is_err());
        assert!(l.add(3, 9, 1, 1u8.into(), "unknown".into()).is_err());
        l.add(2, 9, 1, 40u8.into(), "remaining".into()).unwrap();
        assert_eq!(l.sources[&(9, 1)].1, B::from(100u8));
    }
    #[test]
    fn integer_share_retains_dust_and_rejects_invalid_supply() {
        assert_eq!(
            share(&10u8.into(), &1u8.into(), &3u8.into()).unwrap(),
            B::from(3u8)
        );
        assert!(share(&10u8.into(), &4u8.into(), &3u8.into()).is_err());
        assert!(share(&10u8.into(), &0u8.into(), &0u8.into()).is_err());
        let large = B::from(1u8) << 200usize;
        assert_eq!(share(&large, &2u8.into(), &2u8.into()).unwrap(), large);
    }
    #[test]
    fn theoretical_capacity_uses_each_nonzero_asset_without_fees() {
        assert_eq!(
            capacity_requirement(&B::default(), 2),
            (28_800_000_000u64.into(), 28_800_000_000u64.into())
        );
        assert_eq!(
            capacity_requirement(&6_300_000_000u64.into(), 0),
            (6_300_000_000u64.into(), B::default())
        );
        assert_eq!(
            capacity_requirement(&1_000_000_000u64.into(), 1),
            (14_400_000_000u64.into(), 13_400_000_000u64.into())
        );
    }
    #[test]
    fn versions_cannot_be_mixed() {
        assert!(reconstruct(
            &json!({"protocol":"v1","root":"same"}),
            &json!({"protocol":"v0","root":"same"})
        )
        .is_err());
    }
}
