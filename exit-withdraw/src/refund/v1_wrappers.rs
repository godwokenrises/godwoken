//! Exact-rational static-aToken/BPT beneficiary paths, then one floor per recipient/source lot.
use super::{
    evm::{self, FrozenDb},
    state::{Mapping, Reader},
};
use crate::model::{eth_address, hx};
use anyhow::{ensure, Context, Result};
use num_bigint::BigUint as B;
use serde_json::{json, Value};
use sha3::{Digest, Keccak256};
use std::collections::{BTreeMap, BTreeSet};
fn ar(v: &Value) -> Result<&Vec<Value>> {
    v.as_array().context("Wrapper array")
}
fn s(v: &Value) -> Result<&str> {
    v.as_str().context("Wrapper string")
}
fn n(v: &Value) -> Result<B> {
    Ok(s(v)?.parse()?)
}
fn gcd(mut a: B, mut b: B) -> B {
    while b != B::default() {
        let r = &a % &b;
        a = b;
        b = r;
    }
    a
}
#[derive(Clone, Debug)]
struct Fraction {
    n: B,
    d: B,
}
impl Fraction {
    fn new(n: B, d: B) -> Result<Self> {
        ensure!(d != B::default(), "Zero fraction denominator");
        let g = gcd(n.clone(), d.clone());
        Ok(Self {
            n: n / &g,
            d: d / g,
        })
    }
    fn mul(&self, n: &B, d: &B) -> Result<Self> {
        Self::new(&self.n * n, &self.d * d)
    }
    fn plus(&self, b: &Self) -> Result<Self> {
        Self::new(&self.n * &b.d + &b.n * &self.d, &self.d * &b.d)
    }
}
#[derive(Default)]
struct Graph {
    rules: BTreeMap<String, (B, Vec<(String, B)>)>,
}
impl Graph {
    fn add(&mut self, node: String, den: B, edges: Vec<(String, B)>) -> Result<()> {
        let mut seen = BTreeSet::new();
        ensure!(
            edges.iter().all(|(a, _)| seen.insert(a)),
            "Duplicate rational edge"
        );
        ensure!(
            edges.iter().map(|(_, w)| w).sum::<B>() <= den,
            "Rational weights exceed denominator"
        );
        ensure!(!self.rules.contains_key(&node), "Duplicate rational rule");
        self.rules.insert(node, (den, edges));
        Ok(())
    }
    fn leaves(&self, node: &str) -> Result<BTreeMap<String, Fraction>> {
        let mut out = BTreeMap::new();
        self.walk(
            node,
            Fraction::new(1u8.into(), 1u8.into())?,
            &mut vec![],
            &mut out,
        )?;
        let mut total = Fraction::new(0u8.into(), 1u8.into())?;
        for f in out.values() {
            total = total.plus(f)?;
        }
        ensure!(total.n == total.d, "Rational rights fail conservation");
        Ok(out)
    }
    fn walk(
        &self,
        node: &str,
        amount: Fraction,
        path: &mut Vec<String>,
        out: &mut BTreeMap<String, Fraction>,
    ) -> Result<()> {
        if amount.n == B::default() {
            return Ok(());
        }
        ensure!(
            path.len() < 64 && !path.iter().any(|p| p == node),
            "Rational ownership cycle"
        );
        path.push(node.into());
        if let Some((den, edges)) = self.rules.get(node) {
            if *den == B::default() {
                self.walk(&format!("RESERVE:{node}"), amount, path, out)?;
            } else {
                let mut assigned = B::default();
                for (to, w) in edges {
                    assigned += w;
                    self.walk(to, amount.mul(w, den)?, path, out)?;
                }
                if assigned < *den {
                    self.walk(
                        &format!("RESERVE:{node}"),
                        amount.mul(&(den - assigned), den)?,
                        path,
                        out,
                    )?;
                }
            }
        } else if let Some(old) = out.get_mut(node) {
            *old = old.plus(&amount)?;
        } else {
            out.insert(node.into(), amount);
        }
        path.pop();
        Ok(())
    }
}
fn call(db: &mut FrozenDb, owner: &str, sig: &str, argument: Option<&str>) -> Result<Vec<u8>> {
    let mut input = Keccak256::digest(sig.as_bytes())[..4].to_vec();
    if let Some(a) = argument {
        input.extend_from_slice(&[0; 12]);
        input.extend_from_slice(&eth_address(a)?);
    }
    evm::call(db, eth_address(owner)?, &input).with_context(|| format!("Wrapper {owner} {sig}"))
}
fn address(out: &[u8]) -> Result<String> {
    ensure!(
        out.len() == 32 && out[..12] == [0; 12],
        "Wrapper address word"
    );
    Ok(hx(&out[12..]))
}
pub fn extract(
    reader: &Reader,
    accounts: &[Value],
    state: &Value,
    balancer: &Value,
) -> Result<Value> {
    let Reader::V1(db) = reader else {
        anyhow::bail!("V1 wrappers require V1 DB")
    };
    ensure!(
        state["root"] == reader.root() && balancer["root"] == reader.root(),
        "Mixed wrapper facts"
    );
    let mut getter = FrozenDb::new(db, accounts)?;
    let mut candidates = BTreeSet::new();
    for a in accounts {
        if let Some(a) = a["address"].as_str() {
            candidates.insert(eth_address(a)?);
        }
    }
    for r in ar(&state["rows"])? {
        for m in ar(&r["mappings"])? {
            for h in ar(&m["nonzero_records"])? {
                candidates.insert(eth_address(s(&h["address"])?)?);
            }
        }
    }
    let recipes: Value = serde_json::from_slice(include_bytes!("policy/v1-storage.json"))?;
    for a in ar(&recipes["candidate_hints"])? {
        candidates.insert(eth_address(s(a)?)?);
    }
    let candidates = candidates.into_iter().collect::<Vec<_>>();
    let map = Mapping {
        slot: 52,
        width: 1,
        slot_first: false,
        key_before: None,
        key_after: None,
    };
    let mut wrappers = vec![];
    for id in [11560u32, 11567, 10735, 10737, 11740] {
        let owner = s(&accounts[id as usize]["address"])?;
        let mut holders = vec![];
        let mut total = B::default();
        for chunk in candidates.chunks(256) {
            let keys = chunk
                .iter()
                .map(|a| map.keys(a).map(|k| k[0]))
                .collect::<Result<Vec<_>>>()?;
            let values = reader.storage(id, &keys)?;
            for (owner, raw) in chunk.iter().zip(values) {
                let raw = B::from_bytes_be(&raw);
                if raw != B::default() {
                    total += &raw;
                    holders.push((hx(owner), raw));
                }
            }
        }
        let supply = call(&mut getter, owner, "totalSupply()", None)?;
        ensure!(
            supply.len() == 32
                && B::from_bytes_be(&supply) == total
                && reader.number(id, 54)? == total,
            "Wrapper holder supply completeness {id}"
        );
        for (holder, raw) in &holders {
            let value = call(&mut getter, owner, "balanceOf(address)", Some(holder))?;
            ensure!(
                value.len() == 32 && B::from_bytes_be(&value) == *raw,
                "Wrapper balance getter mismatch"
            );
        }
        let pool = address(&call(&mut getter, owner, "LENDING_POOL()", None)?)?;
        ensure!(
            accounts[10659]["address"] == pool,
            "Wrapper lending pool changed"
        );
        let atoken = address(&call(&mut getter, owner, "ATOKEN()", None)?)?;
        let aid = accounts
            .iter()
            .find(|a| a["address"] == atoken)
            .context("Wrapper aToken identity")?["account_id"]
            .as_u64()
            .context("aToken id")?;
        wrappers.push(json!({"account_id":id,"address":owner,"atoken_account_id":aid,"atoken":atoken,"total_supply_raw":total.to_string(),"holders":holders.iter().map(|(a,q)|json!({"address":a,"raw":q.to_string()})).collect::<Vec<_>>()}));
    }
    let vault = s(&accounts[10711]["address"])?;
    let mut graph = Graph::default();
    let mut cash = BTreeMap::<String, Vec<(String, B)>>::new();
    let mut premints = BTreeMap::new();
    let pools = ar(&balancer["pools"])?
        .iter()
        .filter(|p| p["vault"] == vault)
        .collect::<Vec<_>>();
    for p in &pools {
        let id = p["pool_account_id"].as_u64().context("BPT id")?;
        for t in ar(&p["tokens"])? {
            let token = s(&t["token"])?;
            let q = n(&t["cash_raw"])?;
            if p["address"] == token {
                premints.insert(id, q);
            } else {
                cash.entry(token.into())
                    .or_default()
                    .push((format!("pool:{id}"), q));
            }
        }
    }
    for i in ar(&balancer["internal_balances"])?
        .iter()
        .filter(|i| i["vault"] == vault)
    {
        cash.entry(s(&i["token"])?.into())
            .or_default()
            .push((s(&i["user"])?.into(), n(&i["internal_raw"])?));
    }
    for (id, addr, supply, holders, is_wrapper) in pools
        .iter()
        .map(|p| {
            Ok((
                p["pool_account_id"].as_u64().context("BPT id")?,
                s(&p["address"])?,
                n(&p["total_supply_raw"])?,
                ar(&p["holders"])?,
                false,
            ))
        })
        .chain(wrappers.iter().map(|p| {
            Ok((
                p["account_id"].as_u64().unwrap(),
                s(&p["address"])?,
                n(&p["total_supply_raw"])?,
                ar(&p["holders"])?,
                true,
            ))
        }))
        .collect::<Result<Vec<_>>>()?
    {
        let premint = premints.get(&id).cloned().unwrap_or_default();
        ensure!(premint <= supply, "Wrapper BPT premint");
        let mut edges = vec![];
        for h in holders {
            let owner = s(&h["address"])?;
            let q = n(&h["raw"])?;
            if owner == vault {
                ensure!(q >= premint, "BPT vault premint exceeds holder balance");
                let q = q - &premint;
                if q != B::default() {
                    let key = format!("vault:{addr}");
                    graph.add(
                        key.clone(),
                        q.clone(),
                        cash.get(addr).cloned().unwrap_or_default(),
                    )?;
                    edges.push((key, q));
                }
            } else {
                edges.push((owner.into(), q));
            }
        }
        graph.add(
            format!("{}:{id}", if is_wrapper { "wrapper" } else { "pool" }),
            supply - premint,
            edges,
        )?;
    }
    let atoken = s(&accounts[10687]["address"])?;
    let actual = call(&mut getter, atoken, "balanceOf(address)", Some(vault))?;
    ensure!(actual.len() == 32, "Vault aToken balance width");
    let actual = B::from_bytes_be(&actual);
    let edges = cash.get(atoken).context("Vault aToken pool rights")?;
    ensure!(
        edges.len() == 1 && edges[0].0 == "pool:12760" && edges[0].1 <= actual,
        "Vault aToken recorded backing changed"
    );
    graph.add("wrapper:10711".into(), actual.clone(), edges.clone())?;
    let identities = accounts
        .iter()
        .filter_map(|a| a["address"].as_str().map(|s| (s, a)))
        .collect::<BTreeMap<_, _>>();
    let mut rights = vec![];
    for id in [11560u64, 11567, 10735, 10737, 11740, 10711] {
        let leaves = graph.leaves(&format!("wrapper:{id}"))?;
        let mut rows = vec![];
        for (owner, q) in leaves {
            let a = identities.get(owner.as_str());
            let status = if owner.starts_with("RESERVE:") {
                "RETAIN_UNBOOKED_OR_ROUNDING"
            } else if owner == "0x0000000000000000000000000000000000000000" {
                "RETAIN_LOCKED_ZERO_ADDRESS"
            } else if [
                "0xed612db37b220f3f24ecd924141a900d7bf00c3e",
                "0x7c2977b0cab94e07334ef9741dfd2d6a883096f8",
            ]
            .contains(&owner.as_str())
            {
                "RETAIN_APPROVED_LOW_VALUE_RELAYER"
            } else if a.is_some_and(|a| a["kind"] == "eth_eoa") {
                "REFUND_EOA"
            } else {
                "UNRESOLVED_WRAPPER_BENEFICIARY"
            };
            rows.push(json!({"address":owner,"account_id":a.map(|a|&a["account_id"]),"numerator":q.n.to_string(),"denominator":q.d.to_string(),"status":status}));
        }
        rights.push(json!({"account_id":id,"ckb_only":![11560,11567].contains(&id),"leaves":rows}));
    }
    Ok(
        json!({"root":reader.root(),"wrappers":wrappers,"rights":rights,"vault_atoken_actual_raw":actual.to_string(),"vault_atoken_booked_raw":edges[0].1.to_string(),"complete_refund_calculation":false}),
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn paths_aggregate_before_integer_floor() {
        let mut g = Graph::default();
        g.add(
            "root".into(),
            3u8.into(),
            vec![("a".into(), 1u8.into()), ("b".into(), 1u8.into())],
        )
        .unwrap();
        g.add("a".into(), 1u8.into(), vec![("user".into(), 1u8.into())])
            .unwrap();
        g.add("b".into(), 1u8.into(), vec![("user".into(), 1u8.into())])
            .unwrap();
        let l = g.leaves("root").unwrap();
        assert_eq!(l["user"].n, B::from(2u8));
        assert_eq!(l["user"].d, B::from(3u8));
        assert_eq!(&l["user"].n * 2u8 / &l["user"].d, B::from(1u8));
    }
    #[test]
    fn rational_graph_rejects_cycles_and_overweights() {
        let mut g = Graph::default();
        assert!(g
            .add("a".into(), 1u8.into(), vec![("b".into(), 2u8.into())])
            .is_err());
        g.add("a".into(), 1u8.into(), vec![("b".into(), 1u8.into())])
            .unwrap();
        g.add("b".into(), 1u8.into(), vec![("a".into(), 1u8.into())])
            .unwrap();
        assert!(g.leaves("a").is_err());
    }
}
