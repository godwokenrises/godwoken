//! Balancer custody partitions derived from canonical DB event discovery and deployed getters.
use super::{
    evm::{self, FrozenDb},
    state::Reader,
};
use crate::model::{bytes, eth_address, hx};
use anyhow::{ensure, Context, Result};
use num_bigint::{BigInt, BigUint as B, Sign};
use serde_json::{json, Value};
use sha3::{Digest, Keccak256};
use std::collections::{BTreeMap, BTreeSet};
fn ar(v: &Value) -> Result<&Vec<Value>> {
    v.as_array().context("Balancer array")
}
fn s(v: &Value) -> Result<&str> {
    v.as_str().context("Balancer string")
}
fn n(v: &Value) -> Result<B> {
    Ok(s(v)?.parse()?)
}
fn addr(b: &[u8]) -> Result<String> {
    ensure!(
        b.len() == 32 && b[..12] == [0; 12],
        "Noncanonical ABI address"
    );
    Ok(hx(&b[12..]))
}
fn word(b: &[u8], i: usize) -> Result<&[u8]> {
    b.get(
        i.checked_mul(32).context("ABI overflow")?
            ..(i + 1).checked_mul(32).context("ABI overflow")?,
    )
    .context("Truncated ABI word")
}
fn small(b: &[u8]) -> Result<usize> {
    ensure!(b.len() == 32 && b[..24] == [0; 24], "ABI integer too large");
    Ok(u64::from_be_bytes(b[24..].try_into()?).try_into()?)
}
fn pool_tokens(b: &[u8]) -> Result<Vec<(String, B)>> {
    ensure!(b.len() % 32 == 0 && b.len() >= 160, "Pool tokens ABI");
    let a = small(word(b, 0)?)?;
    let c = small(word(b, 1)?)?;
    ensure!(
        a % 32 == 0 && c % 32 == 0 && a >= 96 && c >= 96 && a != c,
        "Pool tokens offsets"
    );
    let (a, c) = (a / 32, c / 32);
    let count = small(word(b, a)?)?;
    ensure!(
        count <= 256 && count == small(word(b, c)?)?,
        "Pool tokens array length"
    );
    let mut out = vec![];
    let mut seen = BTreeSet::new();
    for i in 0..count {
        let token = addr(word(b, a + 1 + i)?)?;
        ensure!(seen.insert(token.clone()), "Duplicate pool token");
        out.push((token, B::from_bytes_be(word(b, c + 1 + i)?)));
    }
    Ok(out)
}
fn call(db: &mut FrozenDb, target: &str, sig: &str, args: &[u8]) -> Result<Vec<u8>> {
    let mut data = Keccak256::digest(sig.as_bytes())[..4].to_vec();
    data.extend_from_slice(args);
    evm::call(db, eth_address(target)?, &data).with_context(|| format!("Balancer {target} {sig}"))
}
fn address_arg(s: &str) -> Result<Vec<u8>> {
    let mut b = vec![0; 12];
    b.extend_from_slice(&eth_address(s)?);
    Ok(b)
}
pub fn extract(
    reader: &Reader,
    accounts: &[Value],
    ledgers: &Value,
    history: &Value,
) -> Result<Value> {
    let Reader::V1(db) = reader else {
        anyhow::bail!("V1 Balancer requires V1 DB")
    };
    ensure!(history["root"] == reader.root(), "Mixed Balancer history");
    let mut getter = FrozenDb::new(db, accounts)?;
    let ledger = ar(ledgers)?;
    let mut vaults = BTreeSet::new();
    let mut pools = vec![];
    let mut discovered = BTreeSet::new();
    let mut vault_tokens = BTreeSet::new();
    for l in ledger.iter().filter(|l| l["family"] == "balancer_receipt") {
        let id = l["account_id"].as_u64().context("BPT id")?;
        let owner = s(&accounts[id as usize]["address"])?;
        let vault = addr(&call(&mut getter, owner, "getVault()", &[])?)?;
        let pool_id = call(&mut getter, owner, "getPoolId()", &[])?;
        ensure!(pool_id.len() == 32, "Pool id width");
        let registered = call(&mut getter, &vault, "getPool(bytes32)", &pool_id)?;
        ensure!(
            registered.len() == 64 && addr(&registered[..32])? == owner,
            "Pool registration identity mismatch"
        );
        ensure!(
            discovered.insert((vault.clone(), hx(&pool_id))),
            "Duplicate pool registration"
        );
        vaults.insert(vault.clone());
        let token_state = pool_tokens(&call(
            &mut getter,
            &vault,
            "getPoolTokens(bytes32)",
            &pool_id,
        )?)?;
        let mut tokens = vec![];
        for (token, balance) in token_state {
            let mut args = pool_id.clone();
            args.extend_from_slice(&address_arg(&token)?);
            let info = call(
                &mut getter,
                &vault,
                "getPoolTokenInfo(bytes32,address)",
                &args,
            )?;
            ensure!(info.len() == 128, "Pool token info width");
            let cash = B::from_bytes_be(&info[..32]);
            let managed = B::from_bytes_be(&info[32..64]);
            ensure!(&cash + &managed == balance, "Pool balance mismatch");
            ensure!(
                managed == B::default(),
                "Managed pool assets require another adapter"
            );
            vault_tokens.insert((vault.clone(), token.clone()));
            tokens.push(json!({"token":token,"cash_raw":cash.to_string(),"managed_raw":managed.to_string()}));
        }
        let supply = n(&l["supply_raw"])?;
        let observed = call(&mut getter, owner, "totalSupply()", &[])?;
        ensure!(
            observed.len() == 32 && B::from_bytes_be(&observed) == supply,
            "BPT supply changed"
        );
        pools.push(json!({"pool_account_id":id,"pool_id":hx(pool_id),"address":owner,"vault":vault,"tokens":tokens,"total_supply_raw":supply.to_string(),"holders":l["holders"]}));
    }
    let reg_topic = hx(Keccak256::digest(b"PoolRegistered(bytes32,address,uint8)"));
    let internal_topic = hx(Keccak256::digest(
        b"InternalBalanceChanged(address,address,int256)",
    ));
    let mut registrations = BTreeSet::new();
    let mut internal = BTreeMap::<(String, String, String), BigInt>::new();
    for e in ar(&history["events"])? {
        let vault = s(&e["emitter"])?;
        if !vaults.contains(vault) {
            continue;
        }
        let topics = ar(&e["topics"])?;
        if topics[0] == reg_topic {
            ensure!(topics.len() == 3, "Pool registration topics");
            registrations.insert((vault.to_owned(), s(&topics[1])?.to_owned()));
        } else if topics[0] == internal_topic {
            ensure!(topics.len() == 3, "Internal balance topics");
            let owner = addr(&bytes(s(&topics[1])?)?)?;
            let token = addr(&bytes(s(&topics[2])?)?)?;
            let data = bytes(s(&e["data"])?)?;
            ensure!(data.len() == 32, "Internal balance delta width");
            let mut delta = BigInt::from_biguint(Sign::Plus, B::from_bytes_be(&data));
            if data[0] & 128 != 0 {
                delta -= BigInt::from(1u8) << 256;
            }
            *internal.entry((vault.into(), owner, token)).or_default() += delta;
        }
    }
    ensure!(
        registrations == discovered,
        "Balancer registration history does not match pool census"
    );
    let mut internals = vec![];
    for ((vault, owner, token), historical) in internal {
        ensure!(
            historical.sign() != Sign::Minus,
            "Negative reconstructed internal ledger"
        );
        let mut args = address_arg(&owner)?;
        args.extend_from_slice(&super::state::word(64));
        args.extend_from_slice(&super::state::word(1));
        args.extend_from_slice(&address_arg(&token)?);
        let output = call(
            &mut getter,
            &vault,
            "getInternalBalance(address,address[])",
            &args,
        )?;
        ensure!(
            output.len() == 96 && small(word(&output, 0)?)? == 32 && small(word(&output, 1)?)? == 1,
            "Internal balance ABI"
        );
        let current = B::from_bytes_be(word(&output, 2)?);
        ensure!(
            BigInt::from(current.clone()) == historical,
            "Internal event and current DB getter mismatch"
        );
        if current != B::default() {
            vault_tokens.insert((vault.clone(), token.clone()));
            internals.push(json!({"vault":vault,"user":owner,"token":token,"internal_raw":current.to_string()}));
        }
    }
    let mut reconciliation = vec![];
    for (vault, token) in vault_tokens {
        let result = call(
            &mut getter,
            &token,
            "balanceOf(address)",
            &address_arg(&vault)?,
        )?;
        ensure!(result.len() == 32, "Vault ERC20 balance width");
        let balance = B::from_bytes_be(&result);
        let mut pools_sum = B::default();
        for p in &pools {
            if p["vault"] == vault {
                for t in ar(&p["tokens"])? {
                    if t["token"] == token {
                        pools_sum += n(&t["cash_raw"])?;
                    }
                }
            }
        }
        let internal_sum = internals
            .iter()
            .filter(|i| i["vault"] == vault && i["token"] == token)
            .map(|i| n(&i["internal_raw"]))
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .sum::<B>();
        ensure!(
            balance >= &pools_sum + &internal_sum,
            "Vault cash below registered pool and internal rights"
        );
        reconciliation.push(json!({"vault":vault,"token":token,"cash_raw":balance.to_string(),"pool_cash_raw":pools_sum.to_string(),"internal_raw":internal_sum.to_string(),"retained_fees_or_unattributed_raw":(balance-pools_sum-internal_sum).to_string()}));
    }
    Ok(
        json!({"root":reader.root(),"pools":pools,"internal_balances":internals,"reconciliation":reconciliation,"all_registered_pools_discovered":true,"complete_refund_calculation":false}),
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn malformed_dynamic_pool_arrays_fail() {
        assert!(pool_tokens(&[0; 160]).is_err());
        let mut b = vec![0; 224];
        b[..32].copy_from_slice(&super::super::state::word(96));
        b[32..64].copy_from_slice(&super::super::state::word(160));
        b[96..128].copy_from_slice(&super::super::state::word(1));
        b[160..192].copy_from_slice(&super::super::state::word(1));
        assert!(pool_tokens(&b).is_ok());
        b[192..].copy_from_slice(&super::super::state::word(5));
        assert_eq!(pool_tokens(&b).unwrap()[0].1, B::from(5u8));
        b[160..192].copy_from_slice(&super::super::state::word(2));
        assert!(pool_tokens(&b).is_err());
    }
}

/// Original pool-level integer allocation. Later static-wrapper look-through
/// uses its separately approved rational aggregation boundary.
pub fn add_rules(
    graph: &mut super::attribution::Graph,
    facts: &Value,
    aliases: &BTreeMap<String, u64>,
) -> Result<()> {
    use super::attribution::{Edge, Node};
    let token = |s: &str| {
        aliases
            .get(s)
            .map(|id| format!("native:{id}"))
            .unwrap_or_else(|| s.to_owned())
    };
    let pools = ar(&facts["pools"])?;
    for p in pools {
        let id = p["pool_account_id"].as_u64().context("Pool id")?;
        let receipt = s(&p["address"])?;
        let vault = s(&p["vault"])?;
        let premint = ar(&p["tokens"])?
            .iter()
            .find(|t| t["token"] == receipt)
            .map(|t| n(&t["cash_raw"]))
            .transpose()?
            .unwrap_or_default();
        let supply = n(&p["total_supply_raw"])?;
        ensure!(premint <= supply, "Premint exceeds BPT supply");
        let mut accounted = B::default();
        let mut edges = vec![];
        let mut premint_removed = premint == B::default();
        for h in ar(&p["holders"])? {
            let owner = s(&h["address"])?;
            let mut weight = n(&h["raw"])?;
            accounted += &weight;
            if owner == vault {
                ensure!(weight >= premint, "Vault BPT below premint");
                weight -= &premint;
                premint_removed = true;
            }
            if weight != B::default() {
                edges.push(Edge {
                    node: Node {
                        owner: owner.into(),
                        token: receipt.into(),
                    },
                    weight,
                });
            }
        }
        ensure!(
            accounted == supply && premint_removed,
            "BPT holder/premint coverage"
        );
        graph.add(
            Node {
                owner: format!("bpool:{id}"),
                token: "*".into(),
            },
            supply - premint,
            edges,
            "BPT locked shares, fees and rounding",
        )?;
    }
    for r in ar(&facts["reconciliation"])? {
        let vault = s(&r["vault"])?;
        let raw_token = s(&r["token"])?;
        let mut total = n(&r["cash_raw"])?;
        let mut edges = vec![];
        for p in pools.iter().filter(|p| p["vault"] == vault) {
            if let Some(t) = ar(&p["tokens"])?.iter().find(|t| t["token"] == raw_token) {
                let cash = n(&t["cash_raw"])?;
                if p["address"] == raw_token {
                    ensure!(cash <= total, "Vault cash below own-pool premint");
                    total -= cash;
                } else {
                    edges.push(Edge {
                        node: Node {
                            owner: format!("bpool:{}", p["pool_account_id"]),
                            token: "*".into(),
                        },
                        weight: cash,
                    });
                }
            }
        }
        for i in ar(&facts["internal_balances"])?
            .iter()
            .filter(|i| i["vault"] == vault && i["token"] == raw_token)
        {
            edges.push(Edge {
                node: Node {
                    owner: s(&i["user"])?.into(),
                    token: token(raw_token),
                },
                weight: n(&i["internal_raw"])?,
            });
        }
        graph.add(
            Node {
                owner: vault.into(),
                token: token(raw_token),
            },
            total,
            edges,
            "Balancer fees, unattributed custody and rounding",
        )?;
    }
    Ok(())
}
