//! Reviewed receipt layouts, checked against deployed getters executed from DB.
use super::{
    evm::{self, FrozenDb},
    state::Reader,
};
use crate::model::{bytes, eth_address, hx};
use anyhow::{ensure, Context, Result};
use num_bigint::BigUint as B;
use serde::Deserialize;
use serde_json::{json, Value};
use sha3::{Digest, Keccak256};
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Recipe {
    account_id: u32,
    family: String,
    slot: u64,
    slot_first: bool,
    balance_getter: String,
    supply_getter: String,
}
fn number(
    getter: &mut FrozenDb<'_>,
    target: [u8; 20],
    sig: &str,
    owner: Option<[u8; 20]>,
) -> Result<B> {
    let mut data = Keccak256::digest(sig.as_bytes())[..4].to_vec();
    if let Some(owner) = owner {
        data.extend_from_slice(&[0; 12]);
        data.extend_from_slice(&owner);
    }
    let value =
        evm::call(getter, target, &data).with_context(|| format!("{} {sig}", hx(target)))?;
    ensure!(value.len() == 32, "Non-word receipt getter");
    Ok(B::from_bytes_be(&value))
}
pub fn extract(reader: &Reader, accounts: &[Value], state_rows: &[Value]) -> Result<Value> {
    let Reader::V1(db) = reader else {
        anyhow::bail!("V1 ledgers require V1 DB")
    };
    let recipes: Vec<Recipe> = serde_json::from_slice(include_bytes!("policy/v1-ledgers.json"))?;
    let mut getter = FrozenDb::new(db, accounts)?;
    let mut rows = vec![];
    for r in recipes {
        let a = state_rows
            .iter()
            .find(|a| a["account_id"] == r.account_id)
            .context("Missing receipt state")?;
        ensure!(
            accounts[r.account_id as usize]["account_id"] == r.account_id
                && accounts[r.account_id as usize]["address"] == a["address"]
                && accounts[r.account_id as usize]["runtime_hash"] == a["runtime_hash"],
            "Receipt DB identity mismatch"
        );
        let maps = a["mappings"]
            .as_array()
            .context("Receipt mappings")?
            .iter()
            .filter(|m| {
                m["layout"]["slot"] == r.slot
                    && m["layout"]["width"] == 1
                    && m["layout"]["slot_first"] == r.slot_first
                    && m["layout"]["key_before"].is_null()
                    && m["layout"]["key_after"].is_null()
            })
            .collect::<Vec<_>>();
        ensure!(
            maps.len() == 1,
            "Missing or ambiguous reviewed mapping {}",
            r.account_id
        );
        let target = eth_address(a["address"].as_str().context("Receipt address")?)?;
        let supply = number(&mut getter, target, &r.supply_getter, None)?;
        let mut sum = B::default();
        let mut holders = vec![];
        for h in maps[0]["nonzero_records"]
            .as_array()
            .context("Receipt holders")?
        {
            let owner = eth_address(h["address"].as_str().context("Receipt owner")?)?;
            let raw = B::from_bytes_be(&bytes(
                h["fields"][0].as_str().context("Receipt storage amount")?,
            )?);
            ensure!(
                number(&mut getter, target, &r.balance_getter, Some(owner))? == raw,
                "Receipt getter/storage mismatch {}/{}",
                r.account_id,
                hx(owner)
            );
            sum += &raw;
            holders.push(json!({"address":hx(owner),"raw":raw.to_string()}));
        }
        ensure!(
            sum == supply,
            "Receipt supply closure failed {}: {sum} != {supply}",
            r.account_id
        );
        rows.push(json!({"account_id":r.account_id,"family":r.family,"balance_slot":r.slot,"supply_raw":supply.to_string(),"holders":holders,"holder_supply_closure":true,"all_positive_holders_match_deployed_getter":true}));
        eprintln!(
            "v1 receipt {}: deployed getter and full supply match",
            r.account_id
        );
    }
    Ok(json!(rows))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ledger_recipes_have_only_layouts_and_no_computed_values() {
        let r: Vec<Recipe> =
            serde_json::from_slice(include_bytes!("policy/v1-ledgers.json")).unwrap();
        let mut ids = std::collections::BTreeSet::new();
        for a in r {
            assert!(ids.insert(a.account_id));
            assert!(a.balance_getter.ends_with("(address)"));
            assert!(a.supply_getter.ends_with("()"));
        }
        let mut value: Value =
            serde_json::from_slice(include_bytes!("policy/v1-ledgers.json")).unwrap();
        value[0]["supply_raw"] = json!("100");
        assert!(serde_json::from_value::<Vec<Recipe>>(value).is_err());
    }
}
