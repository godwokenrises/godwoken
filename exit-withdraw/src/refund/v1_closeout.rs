//! Exact pool-local loss waterfall. Facts must come from the DB lending adapter.
//! This calculates an accounting trial; provenance and final routing are separate gates.
use anyhow::{ensure, Context, Result};
use num_bigint::BigUint as B;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
fn ar(v: &Value) -> Result<&Vec<Value>> {
    v.as_array().context("Closeout array")
}
fn s(v: &Value) -> Result<&str> {
    v.as_str().context("Closeout string")
}
fn n(v: &Value) -> Result<B> {
    Ok(s(v)?.parse()?)
}
fn diff(a: &B, b: &B) -> B {
    if a > b {
        a - b
    } else {
        B::default()
    }
}
fn treasury_after_users(cash: &B, ordinary: &B, treasury: &B) -> B {
    diff(cash, ordinary).min(treasury.clone())
}
fn allocation(cash: &B, weight: &B, equity: &B, value: &B) -> B {
    let den = equity.max(value);
    if *den == B::default() {
        B::default()
    } else {
        cash * weight / den
    }
}
pub fn calculate(facts: &Value, accounts: &[Value]) -> Result<Value> {
    let markets = ar(&facts["markets"])?;
    let users = ar(&facts["users"])?;
    let mut pools = BTreeMap::new();
    for m in markets {
        let address = s(&m["pool"])?;
        let id = accounts
            .iter()
            .find(|a| a["address"] == address)
            .context("Lending pool identity")?["account_id"]
            .as_u64()
            .context("Pool id")?;
        pools.insert(address, id);
    }
    let mut reports = vec![];
    let mut lots = vec![];
    let mut beneficiaries = vec![];
    let mut market_seen = BTreeSet::new();
    for (pool, pid) in pools {
        ensure!(
            [10285, 10314, 3635, 10201, 4828, 10659, 5790, 5710, 4757].contains(&pid),
            "Unreviewed pool {pid}"
        );
        let treasury = match pid {
            10659 => Some("0x3c98274aff14ee0deb7d230da173b1bcfc14fd48"),
            3635 => Some("0xb8832fbf0f2438efd9fa90d2e17c0454a2daa942"),
            _ => None,
        };
        let mut total = B::default();
        let mut treasury_claim = B::default();
        let mut weights = vec![];
        let mut treasury_rows = 0;
        for u in users.iter().filter(|u| u["pool"] == pool) {
            let mut claims = B::default();
            let mut debts = B::default();
            for p in ar(&u["assets"])? {
                let m = markets
                    .iter()
                    .find(|m| m["account_id"] == p["market_id"])
                    .context("Position market")?;
                ensure!(m["pool"] == pool, "Cross-pool position");
                let price = n(&m["value_per_raw_unit"])?;
                claims += n(&p["claim_raw"])? * &price;
                debts += n(&p["debt_raw"])? * &price;
            }
            ensure!(
                claims == n(&u["claim_value_raw"])? && debts == n(&u["debt_value_raw"])?,
                "User valuation mismatch"
            );
            let weight = diff(&claims, &debts);
            ensure!(
                weight == n(&u["positive_target_value_raw"])?,
                "User net equity mismatch"
            );
            total += &weight;
            if Some(s(&u["address"])?) == treasury {
                ensure!(debts == B::default(), "Treasury has debt");
                treasury_claim += &weight;
                treasury_rows += 1;
            }
            weights.push((s(&u["address"])?, claims, debts, weight));
        }
        ensure!(
            treasury.is_none() || treasury_rows == 1,
            "Treasury census incomplete"
        );
        let mut value = B::default();
        let mut reserve_value = B::default();
        let mut budgets = vec![];
        for m in markets.iter().filter(|m| m["pool"] == pool) {
            let mid = m["account_id"].as_u64().context("Market id")?;
            ensure!(market_seen.insert(mid), "Repeated cash source");
            let cash = n(&m["cash_raw"])?;
            let price = n(&m["value_per_raw_unit"])?;
            let reserves = n(&m["reserves_raw"])?;
            ensure!(reserves <= cash, "Reserve exceeds market cash");
            let assets = &cash + n(&m["debt_total_raw"])?;
            let obligations = n(&m["claim_total_raw"])? + &reserves;
            let unexplained = diff(&assets, &obligations).min(&cash - &reserves);
            let budget = if price == B::default() {
                B::default()
            } else {
                &cash - &unexplained
            };
            value += &budget * &price;
            reserve_value += &reserves * &price;
            budgets.push((mid, m, cash, reserves, unexplained, budget, price));
        }
        let ordinary = &total - &treasury_claim;
        let treasury_remaining = treasury_after_users(&value, &ordinary, &treasury_claim);
        let revised = &ordinary + &treasury_remaining;
        let base_value = &value - &reserve_value;
        let reserve_absorption = reserve_value.clone().min(diff(&total, &base_value));
        let treasury_absorption = &treasury_claim - &treasury_remaining;
        let ignored = [4757, 3635, 10285].contains(&pid);
        for (address, claims, debts, weight) in &mut weights {
            if Some(*address) == treasury {
                *weight = treasury_remaining.clone();
            }
            beneficiaries.push(json!({"pool_account_id":pid,"address":address,"deposit_value_raw":claims.to_string(),"debt_value_raw":debts.to_string(),"effective_equity_raw":weight.to_string(),"excluded_pool":ignored}));
        }
        for (mid, m, cash, reserves, unexplained, budget, price) in budgets {
            let mut assigned = B::default();
            let mut claims = vec![];
            for (owner, _, _, weight) in &weights {
                let q = allocation(&budget, weight, &revised, &value);
                assigned += &q;
                if q != B::default() {
                    claims.push(json!({"address":owner,"raw":q.to_string()}));
                }
            }
            ensure!(
                assigned <= budget && budget <= cash,
                "Closeout source overdraw"
            );
            lots.push(json!({"pool_account_id":pid,"market_account_id":mid,"underlying":m["underlying"],"cash_raw":cash.to_string(),"reserves_raw":reserves.to_string(),"protected_component_raw":unexplained.to_string(),"budget_raw":budget.to_string(),"allocated_trial_raw":assigned.to_string(),"retained_trial_raw":(&cash-&assigned).to_string(),"value_per_raw_unit":price.to_string(),"claims":claims,"excluded_pool":ignored,"approved_refund_raw":if ignored{"0".into()}else{assigned.to_string()}}));
        }
        reports.push(json!({"pool_account_id":pid,"pool":pool,"original_positive_equity_value_raw":total.to_string(),"ordinary_equity_value_raw":ordinary.to_string(),"treasury_equity_value_raw":treasury_claim.to_string(),"available_budget_value_raw":value.to_string(),"reserve_absorption_value_raw":reserve_absorption.to_string(),"treasury_absorption_value_raw":treasury_absorption.to_string(),"treasury_remaining_value_raw":treasury_remaining.to_string(),"ordinary_shortfall_value_raw":diff(&ordinary,&value).to_string(),"revised_denominator_equity_raw":revised.to_string(),"excluded_pool":ignored,"disposition":match pid{4757=>"EXCLUDE_OLD_POOL",3635|10285=>"EXCLUDE_APPROVED_LOW_REMAINING_EQUITY",_=>"ALLOCATE_POOL_LOCAL_NET_EQUITY"}}));
    }
    Ok(
        json!({"root":facts["root"],"pools":reports,"users":beneficiaries,"markets":lots,"complete_refund_calculation":false,"treasury_history_provenance_verified":false,"remaining_gates":["Reauthenticate treasury provenance and collateral semantics","Route contract creditors to final approved beneficiaries and native sources"]}),
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reserves_and_treasury_cannot_reduce_senior_equity_first() {
        assert_eq!(
            treasury_after_users(&90u8.into(), &100u8.into(), &30u8.into()),
            B::default()
        );
        assert_eq!(
            treasury_after_users(&110u8.into(), &100u8.into(), &30u8.into()),
            B::from(10u8)
        );
        assert_eq!(
            treasury_after_users(&150u8.into(), &100u8.into(), &30u8.into()),
            B::from(30u8)
        );
    }
    #[test]
    fn basket_allocation_is_bounded_and_leaves_excess_value() {
        assert_eq!(
            allocation(&10u8.into(), &2u8.into(), &3u8.into(), &2u8.into()),
            B::from(6u8)
        );
        assert_eq!(
            allocation(&10u8.into(), &2u8.into(), &2u8.into(), &4u8.into()),
            B::from(5u8)
        );
        assert_eq!(
            allocation(&10u8.into(), &0u8.into(), &0u8.into(), &0u8.into()),
            B::default()
        );
    }
}
