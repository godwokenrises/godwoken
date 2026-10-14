//! Frozen V1 lending positions reconstructed from deployed getters and DB ledgers.
//! Embedded metadata contains identities only. No RPC or historical numeric input.
use super::{
    evm::{self, FrozenDb},
    state::Reader,
};
use crate::model::{bytes, eth_address};
use anyhow::{ensure, Context, Result};
use num_bigint::{BigInt, BigUint as B};
use serde::Deserialize;
use serde_json::{json, Value};
use sha3::{Digest, Keccak256};
use std::collections::{BTreeMap, BTreeSet};
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Market {
    account_id: u64,
    debt_tokens: Vec<String>,
    family: String,
    oracle: String,
    pool: String,
    token: String,
    underlying: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Price {
    underlying: String,
    symbol: String,
    reference: String,
}
fn ar(v: &Value) -> Result<&Vec<Value>> {
    v.as_array().context("Lending array")
}
fn n(v: &Value) -> Result<B> {
    Ok(v.as_str().context("Lending integer")?.parse()?)
}
fn hexn(v: &Value) -> Result<B> {
    Ok(B::from_bytes_be(&bytes(
        v.as_str().context("Lending word")?,
    )?))
}
struct View<'a> {
    db: FrozenDb<'a>,
}
impl View<'_> {
    fn call(&mut self, target: &str, sig: &str, args: &[u8]) -> Result<Vec<u8>> {
        let mut input = Keccak256::digest(sig.as_bytes())[..4].to_vec();
        input.extend_from_slice(args);
        evm::call(&mut self.db, eth_address(target)?, &input)
            .with_context(|| format!("Lending {target} {sig}"))
    }
    fn number(&mut self, target: &str, sig: &str, owner: Option<&str>) -> Result<B> {
        let mut argument = Vec::new();
        if let Some(owner) = owner {
            argument.extend_from_slice(&[0; 12]);
            argument.extend_from_slice(&eth_address(owner)?);
        }
        let out = self.call(target, sig, &argument)?;
        ensure!(out.len() == 32, "Lending numeric getter width");
        Ok(B::from_bytes_be(&out))
    }
    fn address(&mut self, target: &str, sig: &str) -> Result<String> {
        let out = self.call(target, sig, &[])?;
        ensure!(
            out.len() == 32 && out[..12] == [0; 12],
            "Lending canonical address"
        );
        Ok(crate::model::hx(&out[12..]))
    }
}
#[derive(Default)]
struct Position {
    claims: BTreeMap<u64, B>,
    debts: BTreeMap<u64, B>,
}
pub fn extract(
    reader: &Reader,
    accounts: &[Value],
    state: &Value,
    ledgers: &Value,
) -> Result<Value> {
    let Reader::V1(db) = reader else {
        anyhow::bail!("V1 lending requires V1 DB")
    };
    ensure!(state["root"] == reader.root(), "Mixed lending facts");
    let mut view = View {
        db: FrozenDb::new(db, accounts)?,
    };
    let recipes: Vec<Market> =
        serde_json::from_slice(include_bytes!("policy/v1-lending-markets.json"))?;
    let price_ids: Vec<Price> =
        serde_json::from_slice(include_bytes!("policy/v1-price-identities.json"))?;
    let identities = accounts
        .iter()
        .filter_map(|a| a["address"].as_str().map(|s| (s, a)))
        .collect::<BTreeMap<_, _>>();
    let mut prices = BTreeMap::new();
    let mut observations = vec![];
    for p in price_ids {
        ensure!(p.symbol.len() <= 32, "Price symbol width");
        let mut args = Vec::with_capacity(192);
        args.extend_from_slice(&super::state::word(64));
        args.extend_from_slice(&super::state::word(128));
        args.extend_from_slice(&super::state::word(p.symbol.len() as u64));
        let mut symbol = [0; 32];
        symbol[..p.symbol.len()].copy_from_slice(p.symbol.as_bytes());
        args.extend_from_slice(&symbol);
        args.extend_from_slice(&super::state::word(3));
        let mut usd = [0; 32];
        usd[..3].copy_from_slice(b"USD");
        args.extend_from_slice(&usd);
        let out = view.call(&p.reference, "getReferenceData(string,string)", &args)?;
        ensure!(out.len() == 96, "Band reference result width");
        let rate = B::from_bytes_be(&out[..32]);
        ensure!(rate != B::default(), "Zero approved Band price");
        observations.push(json!({"underlying":p.underlying,"symbol":p.symbol,"reference":p.reference,"rate_raw":rate.to_string(),"base_updated_at":B::from_bytes_be(&out[32..64]).to_string(),"quote_updated_at":B::from_bytes_be(&out[64..]).to_string()}));
        ensure!(
            prices.insert(p.underlying, rate).is_none(),
            "Duplicate price identity"
        );
    }
    let ledgers = ar(ledgers)?;
    let mut positions = BTreeMap::<(String, String), Position>::new();
    let mut markets = vec![];
    let wad = B::from(10u8).pow(18);
    let ray = B::from(10u8).pow(27);
    for m in recipes {
        ensure!(
            accounts[m.account_id as usize]["address"] == m.token,
            "Lending receipt identity changed"
        );
        let aave = m.family == "Aave";
        ensure!(aave || m.family == "Compound", "Unknown lending family");
        ensure!(
            view.address(&m.token, if aave { "POOL()" } else { "comptroller()" })? == m.pool,
            "Lending pool pointer changed"
        );
        ensure!(
            view.address(
                &m.token,
                if aave {
                    "UNDERLYING_ASSET_ADDRESS()"
                } else {
                    "underlying()"
                }
            )? == m.underlying,
            "Lending underlying changed"
        );
        let decimals: u32 = view
            .number(&m.underlying, "decimals()", None)?
            .to_string()
            .parse()?;
        ensure!(decimals <= 18, "Lending decimals");
        let total = view.number(&m.token, "totalSupply()", None)?;
        let unit_price = prices
            .get(&m.underlying)
            .map(|p| p * B::from(10u8).pow(18 - decimals))
            .unwrap_or_default();
        if unit_price == B::default() {
            ensure!(total == B::default(), "Unpriced funded market");
        }
        let cash = view.number(&m.underlying, "balanceOf(address)", Some(&m.token))?;
        let reserves = if aave {
            B::default()
        } else {
            view.number(&m.token, "totalReserves()", None)?
        };
        ensure!(reserves <= cash, "Explicit reserves exceed cash");
        let exchange = if aave {
            wad.clone()
        } else {
            view.number(&m.token, "exchangeRateStored()", None)?
        };
        if !aave {
            ensure!(
                cash == view.number(&m.token, "getCash()", None)?,
                "Underlying cash mismatch"
            );
        }
        let income = if aave {
            view.number(
                &m.pool,
                "getReserveNormalizedIncome(address)",
                Some(&m.underlying),
            )?
        } else {
            B::default()
        };
        let expected = if aave {
            view.number(&m.token, "scaledTotalSupply()", None)?
        } else {
            total.clone()
        };
        let ledger = ledgers.iter().find(|l| l["account_id"] == m.account_id);
        let mut receipt_sum = B::default();
        let mut claim_sum = B::default();
        if let Some(l) = ledger {
            for h in ar(&l["holders"])? {
                let owner = h["address"].as_str().context("Deposit owner")?;
                let receipt = n(&h["raw"])?;
                receipt_sum += &receipt;
                let observed = view.number(&m.token, "balanceOf(address)", Some(owner))?;
                let claim = if aave {
                    let q = (&receipt * &income + &ray / 2u8) / &ray;
                    ensure!(observed == q, "Indexed deposit getter mismatch");
                    q
                } else {
                    ensure!(observed == receipt, "Deposit getter mismatch");
                    &receipt * &exchange / &wad
                };
                claim_sum += &claim;
                positions
                    .entry((m.pool.clone(), owner.into()))
                    .or_default()
                    .claims
                    .insert(m.account_id, claim);
            }
        }
        ensure!(
            receipt_sum == expected,
            "Lending deposit holder completeness {}",
            m.account_id
        );
        let mut debt_sum = B::default();
        let mut debt_total = B::default();
        if aave {
            ensure!(m.debt_tokens.len() == 2, "Aave debt identities");
            for (ix, token) in m.debt_tokens.iter().enumerate() {
                let did = identities.get(token.as_str()).context("Debt identity")?["account_id"]
                    .as_u64()
                    .context("Debt id")?;
                let total = view.number(token, "totalSupply()", None)?;
                debt_total += &total;
                if ix == 0 {
                    ensure!(
                        total == B::default(),
                        "Stable debt requires additional principal adapter"
                    );
                    continue;
                }
                let index = view.number(
                    &m.pool,
                    "getReserveNormalizedVariableDebt(address)",
                    Some(&m.underlying),
                )?;
                let mut scaled = B::default();
                if let Some(l) = ledgers.iter().find(|l| l["account_id"] == did) {
                    for h in ar(&l["holders"])? {
                        let owner = h["address"].as_str().context("Debt owner")?;
                        let q = n(&h["raw"])?;
                        scaled += &q;
                        let owed = view.number(token, "balanceOf(address)", Some(owner))?;
                        ensure!(
                            owed == (&q * &index + &ray / 2u8) / &ray,
                            "Indexed debt mismatch"
                        );
                        debt_sum += &owed;
                        *positions
                            .entry((m.pool.clone(), owner.into()))
                            .or_default()
                            .debts
                            .entry(m.account_id)
                            .or_default() += owed;
                    }
                }
                ensure!(
                    scaled == view.number(token, "scaledTotalSupply()", None)?,
                    "Debt holder completeness"
                );
            }
        } else {
            debt_total = view.number(&m.token, "totalBorrows()", None)?;
            let row = ar(&state["rows"])?
                .iter()
                .find(|r| r["account_id"] == m.account_id);
            if let Some(r) = row {
                let maps = ar(&r["mappings"])?
                    .iter()
                    .filter(|x| {
                        x["layout"]["slot"] == 16
                            && x["layout"]["width"] == 2
                            && x["layout"]["slot_first"] == false
                            && x["layout"]["key_before"].is_null()
                            && x["layout"]["key_after"].is_null()
                    })
                    .collect::<Vec<_>>();
                ensure!(maps.len() == 1, "Compound borrow layout");
                let index = view.number(&m.token, "borrowIndex()", None)?;
                for h in ar(&maps[0]["nonzero_records"])? {
                    let owner = h["address"].as_str().context("Borrower")?;
                    let principal = hexn(&h["fields"][0])?;
                    let previous = hexn(&h["fields"][1])?;
                    ensure!(
                        principal == B::default() || previous != B::default(),
                        "Borrower index zero"
                    );
                    let expected = if principal == B::default() {
                        B::default()
                    } else {
                        principal * &index / previous
                    };
                    let owed =
                        view.number(&m.token, "borrowBalanceStored(address)", Some(owner))?;
                    ensure!(owed == expected, "Compound borrower getter mismatch");
                    debt_sum += &owed;
                    if owed != B::default() {
                        positions
                            .entry((m.pool.clone(), owner.into()))
                            .or_default()
                            .debts
                            .insert(m.account_id, owed);
                    }
                }
            } else {
                ensure!(
                    debt_total == B::default(),
                    "Missing funded Compound borrowers"
                );
            }
        }
        if unit_price == B::default() {
            ensure!(
                debt_total == B::default(),
                "Unpriced funded debt market {}",
                m.account_id
            );
        }
        let claims = &total * &exchange / &wad;
        markets.push(json!({"account_id":m.account_id,"pool":m.pool,"family":m.family,"underlying":m.underlying,"oracle":m.oracle,"decimals":decimals,"value_per_raw_unit":unit_price.to_string(),"cash_raw":cash.to_string(),"reserves_raw":reserves.to_string(),"claim_total_raw":claims.to_string(),"holder_claim_sum_raw":claim_sum.to_string(),"debt_total_raw":debt_total.to_string(),"holder_debt_sum_raw":debt_sum.to_string(),"debt_rounding_difference_raw":(BigInt::from(debt_sum)-BigInt::from(debt_total)).to_string(),"unpriced_cash_must_remain_held":unit_price==B::default()}));
        eprintln!(
            "v1 lending {}: current deposits and debts checked",
            m.account_id
        );
    }
    let mut users = vec![];
    for ((pool, address), p) in positions {
        let mut assets = vec![];
        let mut claim = B::default();
        let mut debt = B::default();
        let ids = p
            .claims
            .keys()
            .chain(p.debts.keys())
            .copied()
            .collect::<std::collections::BTreeSet<_>>();
        for id in ids {
            let m = markets
                .iter()
                .find(|m| m["account_id"] == id)
                .context("Market")?;
            let price = n(&m["value_per_raw_unit"])?;
            let c = p.claims.get(&id).cloned().unwrap_or_default();
            let d = p.debts.get(&id).cloned().unwrap_or_default();
            claim += &c * &price;
            debt += &d * &price;
            assets.push(json!({"market_id":id,"claim_raw":c.to_string(),"debt_raw":d.to_string()}));
        }
        let net = BigInt::from(claim.clone()) - BigInt::from(debt.clone());
        users.push(json!({"pool":pool,"address":address,"claim_value_raw":claim.to_string(),"debt_value_raw":debt.to_string(),"signed_equity_raw":net.to_string(),"positive_target_value_raw":if claim>debt{(claim-debt).to_string()}else{"0".into()},"assets":assets}));
    }
    Ok(
        json!({"root":reader.root(),"markets":markets,"users":users,"prices":observations,"complete_refund_calculation":false,"remaining_gates":["Treasury provenance and first-loss waterfall","Source-preserving final beneficiary allocation"]}),
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn identities_cannot_embed_values() {
        let a: Vec<Market> =
            serde_json::from_slice(include_bytes!("policy/v1-lending-markets.json")).unwrap();
        assert_eq!(a.len(), 44);
        let mut v: Value =
            serde_json::from_slice(include_bytes!("policy/v1-lending-markets.json")).unwrap();
        v[0]["cash_raw"] = json!("100");
        assert!(serde_json::from_value::<Vec<Market>>(v).is_err());
        let _: Vec<Price> =
            serde_json::from_slice(include_bytes!("policy/v1-price-identities.json")).unwrap();
    }
}

/// Recheck the assumptions that allow the approved pool-local waterfall.
/// Historical payloads are canonical-DB evidence, not a replay proof.
pub fn verify_closeout_evidence(
    reader: &Reader,
    accounts: &[Value],
    facts: &Value,
    history: &Value,
) -> Result<Value> {
    let Reader::V1(db) = reader else {
        anyhow::bail!("V1 evidence requires V1 DB")
    };
    ensure!(
        facts["root"] == reader.root()
            && history["root"] == reader.root()
            && history["receipts_scanned"].as_u64().is_some_and(|n| n > 0),
        "Mixed or missing closeout evidence"
    );
    let mut view = View {
        db: FrozenDb::new(db, accounts)?,
    };
    let markets = ar(&facts["markets"])?;
    let users = ar(&facts["users"])?;
    let events = ar(&history["events"])?;
    let transfer = crate::model::hx(Keccak256::digest(b"Transfer(address,address,uint256)"));
    let deposit = crate::model::hx(Keccak256::digest(
        b"Deposit(address,address,address,uint256,uint16)",
    ));
    let word_owner = |s: &str| -> Result<String> {
        let mut b = vec![0; 12];
        b.extend_from_slice(&eth_address(s)?);
        Ok(crate::model::hx(b))
    };
    let mut treasuries = vec![];
    for (pid, owner) in [
        (3635, "0xb8832fbf0f2438efd9fa90d2e17c0454a2daa942"),
        (10659, "0x3c98274aff14ee0deb7d230da173b1bcfc14fd48"),
    ] {
        let pool = accounts[pid]["address"].as_str().context("Treasury pool")?;
        let owner_word = word_owner(owner)?;
        let u = users
            .iter()
            .find(|u| u["pool"] == pool && u["address"] == owner)
            .context("Treasury position")?;
        ensure!(
            n(&u["debt_value_raw"])? == B::default(),
            "Treasury has debt"
        );
        ensure!(
            !events.iter().any(|e| e["emitter"] == pool
                && e["topics"][0] == deposit
                && e["topics"][2] == owner_word),
            "Treasury has user deposits; first-loss classification requires review"
        );
        let mut receipts = vec![];
        let mut minted_events = 0;
        for m in markets.iter().filter(|m| m["pool"] == pool) {
            let mid = m["account_id"].as_u64().context("Market id")?;
            let token = accounts[mid as usize]["address"]
                .as_str()
                .context("Receipt address")?;
            let out = evm::call(&mut view.db, eth_address(token)?, &[0xae, 0x16, 0x73, 0x35])?;
            ensure!(
                out.len() == 32 && crate::model::hx(&out) == owner_word,
                "aToken treasury pointer changed"
            );
            for e in events.iter().filter(|e| {
                e["emitter"] == token && e["topics"][0] == transfer && e["topics"][2] == owner_word
            }) {
                ensure!(
                    e["topics"][1] == crate::model::hx([0; 32]),
                    "Treasury received third-party receipt transfer"
                );
                minted_events += 1;
            }
            receipts.push(mid);
        }
        ensure!(!receipts.is_empty(), "Empty treasury market set");
        treasuries.push(json!({"pool_account_id":pid,"treasury":owner,"market_account_ids":receipts,"incoming_receipt_events_are_mints":true,"mint_events":minted_events,"no_user_deposit_events":true,"no_debt":true}));
    }
    let mut borrowers = vec![];
    for u in users {
        let debt = n(&u["debt_value_raw"])?;
        if debt == B::default() {
            continue;
        }
        let pool = u["pool"].as_str().context("Borrower pool")?;
        let owner = u["address"].as_str().context("Borrower owner")?;
        let group = markets
            .iter()
            .filter(|m| m["pool"] == pool)
            .collect::<Vec<_>>();
        ensure!(!group.is_empty(), "Missing borrower pool");
        let aave = group[0]["family"] == "Aave";
        let mut args = vec![0; 12];
        args.extend_from_slice(&eth_address(owner)?);
        let mut enabled = BTreeSet::new();
        if aave {
            let cfg = view.number(pool, "getUserConfiguration(address)", Some(owner))?;
            for m in &group {
                let underlying = m["underlying"].as_str().context("Underlying")?;
                let mut arg = vec![0; 12];
                arg.extend_from_slice(&eth_address(underlying)?);
                let reserve = view.call(pool, "getReserveData(address)", &arg)?;
                ensure!(reserve.len() == 384, "Aave reserve ABI");
                let index: usize = B::from_bytes_be(&reserve[352..384]).to_string().parse()?;
                ensure!(index < 128, "Aave collateral index");
                if ((&cfg >> (2 * index + 1)) & B::from(1u8)) != B::default() {
                    enabled.insert(m["account_id"].as_u64().unwrap());
                }
            }
        } else {
            let output = view.call(pool, "getAssetsIn(address)", &args)?;
            ensure!(
                output.len() >= 64 && B::from_bytes_be(&output[..32]) == 32u8.into(),
                "Compound collateral array offset"
            );
            let count: usize = B::from_bytes_be(&output[32..64]).to_string().parse()?;
            ensure!(
                count <= 256 && output.len() == 64 + 32 * count,
                "Compound collateral array length"
            );
            for token in output[64..].chunks(32) {
                ensure!(token[..12] == [0; 12], "Collateral address padding");
                let token = crate::model::hx(&token[12..]);
                let m = group
                    .iter()
                    .find(|m| {
                        accounts[m["account_id"].as_u64().unwrap() as usize]["address"] == token
                    })
                    .context("Unknown enabled market")?;
                enabled.insert(m["account_id"].as_u64().unwrap());
            }
        }
        let mut collateral = B::default();
        for p in ar(&u["assets"])? {
            let id = p["market_id"].as_u64().context("Position id")?;
            if enabled.contains(&id) {
                let m = group
                    .iter()
                    .find(|m| m["account_id"] == id)
                    .context("Collateral market")?;
                collateral += n(&p["claim_raw"])? * n(&m["value_per_raw_unit"])?;
            }
        }
        let all = n(&u["claim_value_raw"])?;
        let gap = |c: &B| if debt > *c { &debt - c } else { B::default() };
        ensure!(
            gap(&collateral) == gap(&all),
            "Enabled-only collateral changes borrower shortfall {pool}/{owner}"
        );
        borrowers.push(json!({"pool":pool,"address":owner,"enabled_collateral_value_raw":collateral.to_string(),"all_deposit_value_raw":all.to_string(),"debt_value_raw":debt.to_string(),"same_uncovered_debt_under_both_scopes":true}));
    }
    Ok(
        json!({"root":reader.root(),"treasuries":treasuries,"borrowers":borrowers,"treasury_provenance_verified_against_canonical_db_history":true,"collateral_scope_shortfalls_equal":true,"historical_evidence_limitation":history["limitation"]}),
    )
}
