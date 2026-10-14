//! V1 source graph under construction. Every state quantity is freshly extracted;
//! incomplete protocol coverage remains explicit and is not a public payout plan.
use super::{
    attribution::{Edge, Graph, Node},
    evm::{self, FrozenDb},
    state::Reader,
};
use crate::model::{bytes, eth_address, hx};
use anyhow::{ensure, Context, Result};
use num_bigint::BigUint as B;
use serde_json::{json, Value};
use sha3::{Digest, Keccak256};
use std::collections::{BTreeMap, BTreeSet};
const ROOT: &str = "0x5aedd0572ffb113b8b2ee3c850b1e56aede782a2eb0be41fe4b5f3033495caee";
// Frozen policy scope: native asset identities with approved custodian backing.
// Values are never embedded; all YOK and KOI rights are excluded from refunds.
pub(super) const SUPPORTED: [u64; 15] = [
    1, 34, 36, 38, 40, 42, 56, 68, 102, 104, 106, 110, 114, 120, 161,
];
const CONTROLS: [u64; 9] = [225, 253, 263, 3943, 10506, 11784, 4573, 5793, 4075];
fn ar(v: &Value) -> Result<&Vec<Value>> {
    v.as_array().context("V1 array")
}
fn s(v: &Value) -> Result<&str> {
    v.as_str().context("V1 string")
}
fn id(v: &Value) -> Result<u64> {
    v.as_u64().context("V1 id")
}
fn dec(v: &Value) -> Result<B> {
    Ok(s(v)?.parse()?)
}
fn hexnum(v: &Value) -> Result<B> {
    Ok(B::from_bytes_be(&bytes(s(v)?)?))
}
fn word_address(b: &[u8]) -> Result<String> {
    ensure!(
        b.len() == 32 && b[..12] == [0; 12],
        "Canonical address word"
    );
    Ok(hx(&b[12..]))
}
struct Builder<'a> {
    getter: FrozenDb<'a>,
    accounts: &'a [Value],
    state: &'a Value,
    native: &'a Value,
    graph: Graph,
    aliases: BTreeMap<String, u64>,
}
impl<'a> Builder<'a> {
    fn address(&self, aid: u64) -> Result<String> {
        Ok(s(&self.accounts.get(aid as usize).context("V1 account")?["address"])?.into())
    }
    fn call(&mut self, aid: u64, sig: &str, argument: Option<&str>) -> Result<Vec<u8>> {
        let target = eth_address(&self.address(aid)?)?;
        let mut data = Keccak256::digest(sig.as_bytes())[..4].to_vec();
        if let Some(a) = argument {
            data.extend_from_slice(&[0; 12]);
            data.extend_from_slice(&eth_address(a)?);
        }
        evm::call(&mut self.getter, target, &data)
            .with_context(|| format!("V1 account {aid} {sig}"))
    }
    fn selector_address(&mut self, aid: u64, selector: [u8; 4], owner: &str) -> Result<Vec<u8>> {
        let target = eth_address(&self.address(aid)?)?;
        let mut data = Vec::with_capacity(36);
        data.extend_from_slice(&selector);
        data.extend_from_slice(&[0; 12]);
        data.extend_from_slice(&eth_address(owner)?);
        evm::call(&mut self.getter, target, &data)
            .with_context(|| format!("V1 account {aid} selector {}", hex::encode(selector)))
    }
    fn call_args(&mut self, aid: u64, sig: &str, args: &[u8]) -> Result<Vec<u8>> {
        let target = eth_address(&self.address(aid)?)?;
        let mut data = Keccak256::digest(sig.as_bytes())[..4].to_vec();
        data.extend_from_slice(args);
        evm::call(&mut self.getter, target, &data).with_context(|| format!("V1 {aid} {sig}"))
    }
    fn number(&mut self, aid: u64, sig: &str, argument: Option<&str>) -> Result<B> {
        let b = self.call(aid, sig, argument)?;
        ensure!(b.len() == 32, "V1 numeric getter width");
        Ok(B::from_bytes_be(&b))
    }
    fn indexed(&mut self, aid: u64, sig: &str, index: u64) -> Result<Vec<u8>> {
        let target = eth_address(&self.address(aid)?)?;
        let mut data = Keccak256::digest(sig.as_bytes())[..4].to_vec();
        data.extend_from_slice(&super::state::word(index));
        evm::call(&mut self.getter, target, &data)
            .with_context(|| format!("V1 account {aid} {sig}[{index}]"))
    }
    fn indexed_number(&mut self, aid: u64, sig: &str, index: u64) -> Result<B> {
        let out = self.indexed(aid, sig, index)?;
        ensure!(out.len() == 32, "Indexed numeric getter width");
        Ok(B::from_bytes_be(&out))
    }
    fn getter_address(&mut self, aid: u64, sig: &str) -> Result<String> {
        word_address(&self.call(aid, sig, None)?)
    }
    fn token(&self, address: &str) -> String {
        self.aliases
            .get(address)
            .map(|id| format!("native:{id}"))
            .unwrap_or_else(|| address.into())
    }
    fn mapping(&self, aid: u64, slot: u64, width: u64) -> Result<&'a Vec<Value>> {
        let r = ar(&self.state["rows"])?
            .iter()
            .find(|r| r["account_id"] == aid)
            .context("V1 mapping account")?;
        let maps = ar(&r["mappings"])?
            .iter()
            .filter(|m| {
                m["layout"]["slot"] == slot
                    && m["layout"]["width"] == width
                    && m["layout"]["slot_first"] == false
                    && m["layout"]["key_before"].is_null()
                    && m["layout"]["key_after"].is_null()
            })
            .collect::<Vec<_>>();
        ensure!(maps.len() == 1, "V1 reviewed mapping coverage {aid}/{slot}");
        ar(&maps[0]["nonzero_records"])
    }
    fn rule(
        &mut self,
        owner: String,
        token: String,
        denominator: B,
        holders: Vec<(String, B)>,
        receipt: &str,
        reason: &str,
    ) -> Result<()> {
        self.graph.add(
            Node { owner, token },
            denominator,
            holders
                .into_iter()
                .map(|(owner, weight)| Edge {
                    node: Node {
                        owner,
                        token: receipt.into(),
                    },
                    weight,
                })
                .collect(),
            reason,
        )
    }
    fn native_sources(&self, owner: u64, asset: u64) -> Result<B> {
        Ok(ar(&self.native["balances"])?
            .iter()
            .find(|r| r["account_id"] == owner && r["asset_account_id"] == asset)
            .map(|r| dec(&r["raw_amount"]))
            .transpose()?
            .unwrap_or_default())
    }
}
/// Development allocation graph: full V1 release must add all economic adapters,
/// explicit dispositions and controller verification before exposing generation.
pub fn reconstruct_base(native: &Value, state: &Value, reader: &Reader) -> Result<Value> {
    ensure!(
        native["protocol"] == "v1"
            && state["protocol"] == "v1"
            && native["root"] == ROOT
            && state["root"] == ROOT
            && reader.root() == ROOT,
        "Mixed or unsupported V1 facts"
    );
    let Reader::V1(db) = reader else {
        anyhow::bail!("V1 reader required")
    };
    let accounts = ar(&state["accounts"])?;
    let authorities = super::authority::v1(reader, accounts)?;
    let mut b = Builder {
        getter: FrozenDb::new(db, accounts)?,
        accounts,
        state,
        native,
        graph: Graph::default(),
        aliases: BTreeMap::new(),
    };
    let native_ids = ar(&native["assets"])?
        .iter()
        .map(|a| id(&a["asset_account_id"]))
        .collect::<Result<BTreeSet<_>>>()?;
    for a in accounts.iter().filter(|a| {
        a["runtime_hash"] == "0xde4542f5a5bd32c09cd98e9752281f88900a059aab7ac103edd9df214f136c52"
    }) {
        let aid = id(&a["account_id"])?;
        let sid: u64 = b.number(aid, "sudtId()", None)?.to_string().parse()?;
        if native_ids.contains(&sid) {
            b.aliases.insert(b.address(aid)?, sid);
        }
    }
    for aid in SUPPORTED {
        ensure!(
            native_ids.contains(&aid),
            "Missing approved native asset {aid}"
        );
    }
    let ledgers = ar(&state["native_protocols"]["receipt_ledgers"])?;
    for wid in [151, 180, 232] {
        let l = ledgers
            .iter()
            .find(|l| l["account_id"] == wid)
            .context("WCKB ledger")?;
        let supply = dec(&l["supply_raw"])?;
        ensure!(supply == b.native_sources(wid, 1)?, "WCKB backing mismatch");
        let receipt = b.address(wid)?;
        let owners = ar(&l["holders"])?
            .iter()
            .map(|h| Ok((s(&h["address"])?.into(), dec(&h["raw"])?)))
            .collect::<Result<Vec<_>>>()?;
        b.rule(
            receipt.clone(),
            "native:1".into(),
            supply,
            owners,
            &receipt,
            "wrapped CKB held shares and rounding",
        )?;
    }
    for p in ar(&state["native_protocols"]["pairs"])? {
        let pid = id(&p["account_id"])?;
        let receipt = b.address(pid)?;
        let supply = dec(&p["total_supply"])?;
        let fee = dec(&p["fee_lp_to_mint"])?;
        let mut owners = ar(&p["holders"])?
            .iter()
            .map(|h| Ok((s(&h["address"])?.to_owned(), dec(&h["raw"])?)))
            .collect::<Result<Vec<_>>>()?;
        ensure!(
            owners.iter().map(|(_, q)| q).sum::<B>() == supply,
            "Pair holder closure"
        );
        if fee != B::default() {
            owners.push((format!("reserve:pair-fee:{pid}"), fee.clone()));
        }
        for side in ["token0", "token1"] {
            b.rule(
                receipt.clone(),
                b.token(s(&p[side])?),
                &supply + &fee,
                owners.clone(),
                &receipt,
                "LP locked shares, held rights and rounding",
            )?;
        }
    }
    for pool in ar(&state["native_protocols"]["farms"])? {
        let fid = id(&pool["account_id"])?;
        if fid == 266 {
            continue;
        }
        let token = word_address(&bytes(s(&pool["pool_fields"][0])?)?)?;
        let token_id = accounts
            .iter()
            .find(|a| a["address"] == token)
            .and_then(|a| a["account_id"].as_u64());
        let principal = dec(&pool["principal_or_shares_raw"])?;
        let Some(tid) = token_id else {
            ensure!(principal == B::default(), "Unknown funded farm token");
            continue;
        };
        let owner = b.address(fid)?;
        if fid == 265 && pool["pool_id"] == 1 {
            ensure!(
                principal == B::default(),
                "Reviewed dead farm acquired principal"
            );
            continue;
        }
        let cash = b.number(tid, "balanceOf(address)", Some(&owner))?;
        ensure!(principal <= cash, "Farm principal exceeds custody");
        let token = b.token(&token);
        let owners = ar(&pool["users"])?
            .iter()
            .map(|h| Ok((s(&h["address"])?.into(), dec(&h["raw"])?)))
            .collect::<Result<Vec<_>>>()?;
        b.rule(
            owner,
            token.clone(),
            cash,
            owners,
            &token,
            "farm excess and rounding; pending rewards excluded",
        )?;
    }
    let v266 = ar(&state["native_protocols"]["farms"])?
        .iter()
        .find(|p| p["account_id"] == 266)
        .context("Vault 266")?;
    let owners = ar(&v266["users"])?
        .iter()
        .map(|h| Ok((s(&h["address"])?.into(), dec(&h["raw"])?)))
        .collect::<Result<Vec<_>>>()?;
    let supply = b.number(266, "totalShares()", None)?;
    ensure!(
        owners.iter().map(|(_, n)| n).sum::<B>() == supply,
        "Vault 266 share closure"
    );
    b.rule(
        b.address(266)?,
        "native:128".into(),
        supply,
        owners,
        "native:128",
        "vault share rounding",
    )?;
    for vid in [185, 256] {
        let mut owners = b
            .mapping(vid, 1, 4)?
            .iter()
            .map(|h| Ok((s(&h["address"])?.to_owned(), hexnum(&h["fields"][0])?)))
            .collect::<Result<Vec<_>>>()?;
        let supply = b.number(vid, "totalShares()", None)?;
        ensure!(
            owners.iter().map(|(_, n)| n).sum::<B>() == supply,
            "Vault share closure {vid}"
        );
        // Vault 256 first floors each user's gross NAV entitlement, then
        // routes physical source lots against NAV. Preserve both boundaries.
        let denominator = if vid == 256 {
            let nav = b.number(vid, "balanceOf()", None)?;
            ensure!(
                supply != B::default() || nav == B::default(),
                "Vault NAV without shares"
            );
            for (_, q) in &mut owners {
                *q = if supply == B::default() {
                    B::default()
                } else {
                    &*q * &nav / &supply
                };
            }
            nav
        } else {
            supply
        };
        b.rule(
            b.address(vid)?,
            "native:128".into(),
            denominator,
            owners,
            "native:128",
            "vault share rounding",
        )?;
    }
    // Each stable pool's reviewed token count is part of its pinned layout.
    // Its amounts, token pointers and LP ownership are freshly read.
    for (pid, lid, count) in [
        (2733, 2732, 2),
        (2736, 2735, 2),
        (2739, 2738, 2),
        (2742, 2741, 2),
        (4184, 4185, 3),
        (4261, 4262, 2),
        (4316, 4317, 2),
    ] {
        let owner = b.address(pid)?;
        let receipt = b.address(lid)?;
        let ledger = ledgers
            .iter()
            .find(|l| l["account_id"] == lid)
            .context("Stable LP ledger")?;
        let supply = dec(&ledger["supply_raw"])?;
        let holders = ar(&ledger["holders"])?
            .iter()
            .map(|h| Ok((s(&h["address"])?.to_owned(), dec(&h["raw"])?)))
            .collect::<Result<Vec<_>>>()?;
        for i in 0..count {
            let curve = pid < 4000;
            let token = word_address(&b.indexed(
                pid,
                if curve {
                    "coins(uint256)"
                } else {
                    "getToken(uint8)"
                },
                i,
            )?)?;
            let balance = b.indexed_number(
                pid,
                if curve {
                    "balances(uint256)"
                } else {
                    "getTokenBalance(uint8)"
                },
                i,
            )?;
            let admin = b.indexed_number(
                pid,
                if curve {
                    "admin_balances(uint256)"
                } else {
                    "getAdminBalance(uint256)"
                },
                i,
            )?;
            let tid = id(&accounts
                .iter()
                .find(|a| a["address"] == token)
                .context("Stable token identity")?["account_id"])?;
            let cash = b.number(tid, "balanceOf(address)", Some(&owner))?;
            ensure!(
                cash >= &balance + &admin,
                "Stable LP and admin balances exceed physical cash {pid}/{i}"
            );
            let token = b.token(&token);
            let node = format!("stable:{pid}:{i}");
            b.rule(
                owner.clone(),
                token.clone(),
                cash,
                vec![(node.clone(), balance)],
                &token,
                "stable admin and unallocated reserve",
            )?;
            b.rule(
                node,
                token,
                supply.clone(),
                holders.clone(),
                &receipt,
                "stable LP rounding",
            )?;
        }
    }
    for iid in [5731, 5732, 11852, 11853] {
        let owner = b.address(iid)?;
        let token = b.address(187)?;
        ensure!(
            b.getter_address(iid, "depositToken()")? == token,
            "IFO deposit token changed"
        );
        ensure!(
            b.number(iid, "isEnded()", None)? == 1u8.into(),
            "IFO not ended"
        );
        let total = b.number(iid, "totalAmount()", None)?;
        let raising = b.number(iid, "raisingAmount()", None)?;
        let offering = b.number(iid, "offeringAmount()", None)?;
        let users = b.mapping(iid, 7, 2)?;
        let mut deposits = B::default();
        let mut holders = vec![];
        for u in users {
            let address = s(&u["address"])?;
            let deposit = hexnum(&u["fields"][0])?;
            let harvested = hexnum(&u["fields"][1])?;
            ensure!(harvested <= 1u8.into(), "Nonboolean IFO harvested flag");
            let info = b.call(iid, "userInfo(address)", Some(address))?;
            ensure!(
                info.len() == 64
                    && B::from_bytes_be(&info[..32]) == deposit
                    && B::from_bytes_be(&info[32..]) == harvested,
                "IFO user storage/getter mismatch"
            );
            deposits += &deposit;
            if harvested == B::default() {
                let (allocation, offered, refund) =
                    ifo_claim(&deposit, &total, &raising, &offering)?;
                // This deployment exposes these reviewed selectors, not the
                // commonly named getRefundingAmount getter of other IFOs.
                let result = b.selector_address(iid, [0x9e, 0xc9, 0x49, 0x8d], address)?;
                ensure!(
                    result.len() == 64
                        && B::from_bytes_be(&result[..32]) == offered
                        && B::from_bytes_be(&result[32..]) == refund,
                    "IFO refund formula mismatch"
                );
                let result = b.selector_address(iid, [0xba, 0x91, 0x2a, 0xc7], address)?;
                ensure!(
                    result.len() == 32 && B::from_bytes_be(&result) == allocation,
                    "IFO allocation formula mismatch"
                );
                if refund != B::default() {
                    holders.push((address.to_owned(), refund));
                }
            }
        }
        ensure!(deposits == total, "IFO full historical deposit closure");
        let cash = b.number(187, "balanceOf(address)", Some(&owner))?;
        let owed = holders.iter().map(|(_, q)| q).sum::<B>();
        b.rule(
            owner,
            token.clone(),
            cash.max(owed),
            holders,
            &token,
            "approved four IFO pool-local haircut and rounding",
        )?;
    }
    // Reviewed reciprocal vault/strategy relations. These are identities,
    // not saved balances: every pointer, share and supply is read from the DB.
    for (vid, sid, pid) in [
        (1791, 1792, 187),
        (1810, 1811, 187),
        (2069, 2070, 186),
        (2075, 2076, 1438),
        (2314, 2315, 1434),
        (2318, 2319, 1439),
        (2356, 2357, 1433),
        (3171, 3172, 187),
        (3173, 3174, 186),
        (3175, 3176, 1438),
        (3177, 3178, 1434),
        (3179, 3180, 1439),
        (3181, 3182, 1433),
        (3242, 3243, 1437),
        (3244, 3245, 1435),
        (3455, 3456, 1437),
        (3460, 3461, 1437),
        (3462, 3463, 1435),
        (3474, 3475, 1437),
        (3476, 3477, 1435),
        (4223, 4224, 1437),
    ] {
        let vault = b.address(vid)?;
        let strategy = b.address(sid)?;
        let token = b.address(pid)?;
        ensure!(
            b.getter_address(vid, "strategy()")? == strategy
                && b.getter_address(sid, "vault()")? == vault,
            "Nonreciprocal vault strategy {vid}/{sid}"
        );
        ensure!(
            b.getter_address(vid, "want()")? == token && b.getter_address(sid, "want()")? == token,
            "Vault strategy token mismatch {vid}/{sid}"
        );
        let owners = b
            .mapping(vid, 101, 1)?
            .iter()
            .map(|h| Ok((s(&h["address"])?.to_owned(), hexnum(&h["fields"][0])?)))
            .collect::<Result<Vec<_>>>()?;
        let supply = b.number(vid, "totalSupply()", None)?;
        ensure!(
            owners.iter().map(|(_, q)| q).sum::<B>() == supply,
            "Vault holder closure {vid}"
        );
        for (address, q) in &owners {
            ensure!(
                b.number(vid, "balanceOf(address)", Some(address))? == *q,
                "Vault balance getter disagreement {vid}"
            );
        }
        b.rule(
            strategy,
            token.clone(),
            1u8.into(),
            vec![(vault.clone(), 1u8.into())],
            &token,
            "strategy principal to reciprocal vault",
        )?;
        b.rule(
            vault,
            token.clone(),
            supply,
            owners,
            &token,
            "vault share rounding",
        )?;
    }
    // Staking routes only the deposited token. Reward balances stay separate.
    for ledger in ledgers.iter().filter(|l| {
        l["family"] == "locked_staking"
            || [2734, 2737, 2740, 2743]
                .iter()
                .any(|id| l["account_id"] == *id)
    }) {
        let aid = id(&ledger["account_id"])?;
        let signature = if ledger["family"] == "locked_staking" {
            "stakedTokenAddress()"
        } else {
            "stakingToken()"
        };
        let token = b.getter_address(aid, signature)?;
        let tid = accounts
            .iter()
            .find(|a| a["address"] == token)
            .context("Unknown staking token")?["account_id"]
            .as_u64()
            .context("Staking token id")?;
        let owner = b.address(aid)?;
        let cash = b.number(tid, "balanceOf(address)", Some(&owner))?;
        let principal = dec(&ledger["supply_raw"])?;
        ensure!(principal <= cash, "Staking principal exceeds cash {aid}");
        let holders = ar(&ledger["holders"])?
            .iter()
            .map(|h| Ok((s(&h["address"])?.to_owned(), dec(&h["raw"])?)))
            .collect::<Result<Vec<_>>>()?;
        let token = b.token(&token);
        b.rule(
            owner,
            token.clone(),
            cash,
            holders,
            &token,
            "staking surplus and rounding; rewards excluded",
        )?;
    }
    for (aid, pid, slot) in [
        (5824, 5822, 19),
        (5828, 5822, 19),
        (12419, 12279, 12),
        (12601, 12382, 12),
        (13032, 12219, 12),
    ] {
        let owner = b.address(aid)?;
        let token = b.address(pid)?;
        let locked = slot == 19;
        ensure!(
            b.getter_address(
                aid,
                if locked {
                    "stakedTokenAddress()"
                } else {
                    "stakingToken()"
                }
            )? == token,
            "Additional staking token identity {aid}"
        );
        let holders = b
            .mapping(aid, slot, 1)?
            .iter()
            .map(|h| Ok((s(&h["address"])?.to_owned(), hexnum(&h["fields"][0])?)))
            .collect::<Result<Vec<_>>>()?;
        let principal = b.number(
            aid,
            if locked {
                "totalSupplies()"
            } else {
                "totalSupply()"
            },
            None,
        )?;
        ensure!(
            holders.iter().map(|(_, q)| q).sum::<B>() == principal,
            "Additional staking full holder closure {aid}"
        );
        for (address, q) in &holders {
            ensure!(
                b.number(
                    aid,
                    if locked {
                        "supplyAmount(address)"
                    } else {
                        "balanceOf(address)"
                    },
                    Some(address)
                )? == *q,
                "Additional staking getter mismatch {aid}"
            );
        }
        let cash = b.number(pid, "balanceOf(address)", Some(&owner))?;
        ensure!(
            principal <= cash,
            "Additional staking backing shortfall {aid}"
        );
        b.rule(
            owner,
            token.clone(),
            cash,
            holders,
            &token,
            "additional staking principal rounding",
        )?;
    }
    if let Some(balancer) = state["native_protocols"].get("balancer") {
        ensure!(balancer["root"] == ROOT, "Mixed Balancer root");
        super::v1_balancer::add_rules(&mut b.graph, balancer, &b.aliases)?;
    }
    // Reviewed custom principal ledgers. Every positive field is checked
    // against its deployed getter; rewards and monetary control totals are not added.
    let row183 = ar(&state["rows"])?
        .iter()
        .find(|r| r["account_id"] == 183)
        .context("Principal farm 183")?;
    let map183 = ar(&row183["mappings"])?
        .iter()
        .find(|m| m["layout"]["slot"] == 7 && m["layout"]["key_before"] == "0x00")
        .context("Principal farm layout")?;
    ensure!(
        b.number(183, "poolLength()", None)? == 1u8.into(),
        "Unexpected additional farm pools"
    );
    let pool = b.indexed(183, "poolInfo(uint256)", 0)?;
    ensure!(
        pool.len() >= 32 && b.token(&word_address(&pool[..32])?) == "native:128",
        "Principal farm token changed"
    );
    let mut holders = vec![];
    for h in ar(&map183["nonzero_records"])? {
        let owner = s(&h["address"])?;
        let mut args = vec![0; 44];
        args.extend_from_slice(&eth_address(owner)?);
        let result = b.call_args(183, "userInfo(uint256,address)", &args)?;
        ensure!(
            result.len() == 64
                && B::from_bytes_be(&result[..32]) == hexnum(&h["fields"][0])?
                && B::from_bytes_be(&result[32..]) == hexnum(&h["fields"][1])?,
            "Farm principal getter mismatch"
        );
        holders.push((owner.to_owned(), hexnum(&h["fields"][0])?));
    }
    b.rule(
        b.address(183)?,
        "native:128".into(),
        b.native_sources(183, 128)?,
        holders,
        "native:128",
        "custom farm principal residual",
    )?;
    let row = ar(&state["rows"])?
        .iter()
        .find(|r| r["account_id"] == 11919)
        .context("Custody ledger 11919")?;
    for m in ar(&row["mappings"])?
        .iter()
        .filter(|m| m["layout"]["slot"] == 2 && m["layout"]["key_before"].is_string())
    {
        let raw_token = s(&m["layout"]["key_before"])?;
        let records = ar(&m["nonzero_records"])?;
        if records.is_empty() {
            continue;
        }
        let tid = id(&accounts
            .iter()
            .find(|a| a["address"] == raw_token)
            .context("Custody token identity")?["account_id"])?;
        let owner = b.address(11919)?;
        let cash = b.number(tid, "balanceOf(address)", Some(&owner))?;
        let mut holders = vec![];
        for h in records {
            let holder = s(&h["address"])?;
            let mut args = vec![0; 12];
            args.extend_from_slice(&eth_address(raw_token)?);
            args.extend_from_slice(&[0; 12]);
            args.extend_from_slice(&eth_address(holder)?);
            let result = b.call_args(11919, "balances(address,address)", &args)?;
            let amount = hexnum(&h["fields"][0])?;
            ensure!(
                result.len() == 32 && B::from_bytes_be(&result) == amount,
                "Custom custody getter mismatch"
            );
            holders.push((holder.to_owned(), amount));
        }
        let token = b.token(raw_token);
        b.rule(
            owner,
            token.clone(),
            cash,
            holders,
            &token,
            "custom custody principal residual",
        )?;
    }
    for dao in [135, 4502] {
        let owner = b.address(dao)?;
        let shares = b.number(dao, "totalShares()", None)?;
        let loot = b.number(dao, "totalLoot()", None)?;
        let mut members = vec![];
        let mut sum_shares = B::default();
        let mut sum_loot = B::default();
        for h in b.mapping(dao, 18, 6)? {
            let holder = s(&h["address"])?;
            let fields = ar(&h["fields"])?;
            let result = b.call(dao, "members(address)", Some(holder))?;
            ensure!(
                result.len() == 192
                    && result
                        .chunks(32)
                        .zip(fields)
                        .all(|(w, v)| bytes(v.as_str().unwrap_or("")).is_ok_and(|x| x == w)),
                "DAO member getter mismatch"
            );
            let sh = hexnum(&fields[1])?;
            let lo = hexnum(&fields[2])?;
            sum_shares += &sh;
            sum_loot += &lo;
            if &sh + &lo != B::default() {
                let eligible = b.call_args(dao, "canRagequit(uint256)", &bytes(s(&fields[4])?)?)?;
                ensure!(
                    eligible.len() == 32 && B::from_bytes_be(&eligible) == 1u8.into(),
                    "DAO member exit blocked"
                );
            }
            members.push((holder.to_owned(), sh + lo));
        }
        ensure!(
            sum_shares == shares && sum_loot == loot,
            "DAO share/loot holder closure"
        );
        let token_count: u64 = b
            .number(dao, "getTokenCount()", None)?
            .to_string()
            .parse()?;
        ensure!(token_count <= 16, "Unexpected DAO token count");
        for i in 0..token_count {
            let raw_token = word_address(&b.indexed(dao, "approvedTokens(uint256)", i)?)?;
            let tid = id(&accounts
                .iter()
                .find(|a| a["address"] == raw_token)
                .context("DAO token")?["account_id"])?;
            let row = ar(&state["rows"])?
                .iter()
                .find(|r| r["account_id"] == dao)
                .context("DAO row")?;
            let m = ar(&row["mappings"])?
                .iter()
                .find(|m| m["layout"]["slot"] == 13 && m["layout"]["key_after"] == raw_token)
                .context("DAO token mapping")?;
            let cash = b.number(tid, "balanceOf(address)", Some(&owner))?;
            let token = b.token(&raw_token);
            let node = format!("guild:{dao}:{i}");
            let mut edges = vec![];
            let mut total = B::default();
            let mut ledger_total = None;
            for h in ar(&m["nonzero_records"])? {
                let holder = s(&h["address"])?;
                let amount = hexnum(&h["fields"][0])?;
                let mut args = vec![0; 12];
                args.extend_from_slice(&eth_address(holder)?);
                args.extend_from_slice(&[0; 12]);
                args.extend_from_slice(&eth_address(&raw_token)?);
                let out = b.call_args(dao, "userTokenBalances(address,address)", &args)?;
                ensure!(
                    out.len() == 32 && B::from_bytes_be(&out) == amount,
                    "DAO internal balance getter mismatch"
                );
                if holder == format!("0x{:040x}", 0xbabe) {
                    ledger_total = Some(amount);
                    continue;
                }
                total += &amount;
                if holder == format!("0x{:040x}", 0xbeef) {
                    continue;
                }
                edges.push((
                    if holder == format!("0x{:040x}", 0xdead) {
                        node.clone()
                    } else {
                        holder.into()
                    },
                    amount,
                ));
            }
            ensure!(
                ledger_total.unwrap_or_default() == total && total <= cash,
                "DAO internal ledger conservation"
            );
            b.rule(
                owner.clone(),
                token.clone(),
                cash,
                edges,
                &token,
                "DAO escrow and unallocated balances",
            )?;
            b.rule(
                node,
                token.clone(),
                &shares + &loot,
                members.clone(),
                &token,
                "DAO shares/loot rounding",
            )?;
        }
    }
    let owner = b.address(4572)?;
    let wckb = b.address(180)?;
    let cash = b.number(180, "balanceOf(address)", Some(&owner))?;
    let mut claims = vec![];
    let mut sum = B::default();
    for h in b.mapping(4572, 6, 1)? {
        let holder = s(&h["address"])?;
        let amount = hexnum(&h["fields"][0])?;
        ensure!(
            b.number(4572, "pendingRevenue(address)", Some(holder))? == amount,
            "Marketplace pending revenue mismatch"
        );
        sum += &amount;
        claims.push((
            if holder == b.address(263)? {
                holder.into()
            } else {
                format!("reserve:marketplace:{holder}")
            },
            amount,
        ));
    }
    ensure!(sum == cash, "Marketplace holder closure");
    b.rule(
        owner,
        wckb.clone(),
        cash,
        claims,
        &wckb,
        "marketplace residual",
    )?;
    let mut excluded_lending_markets = BTreeSet::new();
    let mut lending_markets = BTreeSet::new();
    if let Some(closeout) = state["native_protocols"].get("lending_closeout") {
        ensure!(
            closeout["root"] == ROOT
                && state["native_protocols"]["lending_evidence"]
                    ["treasury_provenance_verified_against_canonical_db_history"]
                    == true
                && state["native_protocols"]["lending_evidence"]
                    ["collateral_scope_shortfalls_equal"]
                    == true,
            "Lending closeout evidence incomplete"
        );
        for market in ar(&closeout["markets"])? {
            let mid = id(&market["market_account_id"])?;
            lending_markets.insert(mid);
            if market["excluded_pool"] == true {
                excluded_lending_markets.insert(mid);
                continue;
            }
            let owner = b.address(mid)?;
            let token = b.token(s(&market["underlying"])?);
            let holders = ar(&market["claims"])?
                .iter()
                .map(|c| Ok((s(&c["address"])?.to_owned(), dec(&c["raw"])?)))
                .collect::<Result<Vec<_>>>()?;
            b.rule(
                owner.clone(),
                token,
                dec(&market["cash_raw"])?,
                holders,
                &owner,
                "pool-local net equity: protected component and rounding",
            )?;
        }
    }
    let identities = accounts
        .iter()
        .filter_map(|a| a["address"].as_str().map(|addr| (addr, a)))
        .collect::<BTreeMap<_, _>>();
    let mut lots = vec![];
    for source in ar(&native["balances"])? {
        let aid = id(&source["asset_account_id"])?;
        let owner = s(&source["registry_address"])?;
        let q = dec(&source["raw_amount"])?;
        let result = b.graph.allocate(
            Node {
                owner: owner.into(),
                token: format!("native:{aid}"),
            },
            q,
            |node| {
                if !SUPPORTED.contains(&aid) {
                    return Some("EXCLUDED_ASSET_SCOPE".into());
                }
                if node.owner.starts_with("reserve:") {
                    return Some("RETAINED_PROTOCOL".into());
                }
                let a = identities.get(node.owner.as_str());
                let owner_id = a.and_then(|a| a["account_id"].as_u64());
                if source["account_id"]
                    .as_u64()
                    .is_some_and(|id| lending_markets.contains(&id))
                    && owner_id
                        .is_some_and(|id| [11560, 11567, 10735, 10737, 11740, 10711].contains(&id))
                {
                    return Some("PENDING_RATIONAL_LENDING_WRAPPER".into());
                }
                if owner_id.is_some_and(|id| excluded_lending_markets.contains(&id)) {
                    return Some("RETAINED_APPROVED_EXCLUDED_LENDING_POOL".into());
                }
                if owner_id == Some(264) && aid == 128 {
                    return Some("EXCLUDE_PENDING_BONUS_RESERVE".into());
                }
                if owner_id == Some(5707) {
                    return Some("EXCLUDED_ACCOUNT".into());
                }
                if matches!(
                    node.owner.as_str(),
                    "0x0000000000000000000000000000000000000000"
                        | "0x000000000000000000000000000000000000dead"
                ) {
                    return Some("RETAINED_LOCKED_ADDRESS".into());
                }
                if a.is_some_and(|a| a["kind"] == "eth_eoa") {
                    return Some("REFUND_EOA".into());
                }
                if owner_id.is_some_and(|id| CONTROLS.contains(&id)) {
                    return Some("REFUND_CONTROL".into());
                }
                None
            },
        )?;
        for lot in result {
            lots.push(json!({"source_account_id":source["account_id"],"asset_id":aid,"beneficiary_address":lot.node.owner,"entitlement_raw":lot.amount.to_string(),"status":lot.status,"path":lot.path.iter().map(|n|format!("{}:{}",n.owner,n.token)).collect::<Vec<_>>() }));
        }
    }
    if let Some(rights) = state["native_protocols"].get("wrapper_rights") {
        ensure!(rights["root"] == ROOT, "Mixed wrapper rights root");
        let mut expanded = vec![];
        for lot in lots {
            if lot["status"] != "PENDING_RATIONAL_LENDING_WRAPPER" {
                expanded.push(lot);
                continue;
            }
            let account = identities
                .get(s(&lot["beneficiary_address"])?)
                .context("Wrapper identity")?;
            let right = ar(&rights["rights"])?
                .iter()
                .find(|r| r["account_id"] == account["account_id"])
                .context("Wrapper rights missing")?;
            let mut lot = lot;
            if right["ckb_only"] == true && lot["asset_id"] != 1 {
                lot["status"] = json!("RETAINED_APPROVED_SMALL_NON_CKB_RIGHT");
                expanded.push(lot);
                continue;
            }
            let amount = dec(&lot["entitlement_raw"])?;
            let mut assigned = B::default();
            for leaf in ar(&right["leaves"])? {
                let den = dec(&leaf["denominator"])?;
                ensure!(den != B::default(), "Zero wrapper denominator");
                let q = &amount * dec(&leaf["numerator"])? / den;
                assigned += &q;
                if q == B::default() {
                    continue;
                }
                let mut row = lot.clone();
                row["beneficiary_address"] = leaf["address"].clone();
                row["entitlement_raw"] = json!(q.to_string());
                row["status"] = leaf["status"].clone();
                row["rational_wrapper_account_id"] = account["account_id"].clone();
                row["rational_numerator"] = leaf["numerator"].clone();
                row["rational_denominator"] = leaf["denominator"].clone();
                expanded.push(row);
            }
            ensure!(assigned <= amount, "Wrapper source overdraw");
            if assigned < amount {
                lot["entitlement_raw"] = json!((amount - assigned).to_string());
                lot["status"] = json!("RETAINED_RATIONAL_WRAPPER_ROUNDING");
                expanded.push(lot);
            }
        }
        lots = expanded;
    }
    for lot in &mut lots {
        if lot["status"] == "UNRESOLVED" {
            let address = s(&lot["beneficiary_address"])?;
            let owner = identities
                .get(address)
                .and_then(|a| a["account_id"].as_u64());
            let status = super::v1_disposition::residual(owner, id(&lot["asset_id"])?, address)
                .with_context(|| format!("Unreviewed funded residual {owner:?} {address}"))?;
            lot["status"] = json!(status);
        }
        ensure!(
            !s(&lot["status"])?.contains("UNRESOLVED")
                && !s(&lot["status"])?.starts_with("PENDING_"),
            "Incomplete V1 beneficiary route"
        );
    }
    Ok(
        json!({"protocol":"v1","root":ROOT,"complete_refund_calculation":true,"source_lots":lots,"authorities":authorities,"remaining_gates":[]}),
    )
}

fn ifo_claim(deposit: &B, total: &B, raising: &B, offering: &B) -> Result<(B, B, B)> {
    ensure!(
        *total != B::default() && *raising != B::default() && deposit <= total,
        "Invalid IFO accounting"
    );
    let scale = B::from(10u8).pow(12);
    let allocation = deposit * &scale / total;
    if total > raising {
        let accepted = &allocation * raising / &scale;
        ensure!(accepted <= *deposit, "IFO accepted exceeds deposited");
        Ok((
            allocation.clone(),
            &allocation * offering / &scale,
            deposit - accepted,
        ))
    } else {
        Ok((allocation, deposit * offering / raising, B::default()))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ifo_refund_preserves_deployed_double_floor() {
        let (share, offered, refund) =
            ifo_claim(&2u8.into(), &3u8.into(), &2u8.into(), &10u8.into()).unwrap();
        assert_eq!(share, B::from(666666666666u64));
        assert_eq!(offered, B::from(6u8));
        assert_eq!(refund, B::from(1u8));
        assert_eq!(
            ifo_claim(&1u8.into(), &2u8.into(), &3u8.into(), &9u8.into())
                .unwrap()
                .2,
            B::default()
        );
        assert!(ifo_claim(&4u8.into(), &3u8.into(), &2u8.into(), &1u8.into()).is_err());
        assert!(ifo_claim(&0u8.into(), &0u8.into(), &2u8.into(), &1u8.into()).is_err());
    }
}

pub fn authorities(reader: &Reader, accounts: &[Value]) -> Result<Value> {
    super::authority::v1(reader, accounts)
}

/// Production DB-only pipeline. No report/facts file is accepted as an input.
pub fn reconstruct(db: &std::path::Path) -> Result<Value> {
    use crate::settlement::adapter::Protocol;
    let native = crate::asset_query::l2_db(Protocol::V1, db, None)?;
    let mut state = super::state::extract(Protocol::V1, db)?;
    let reader = Reader::open(Protocol::V1, db)?;
    let accounts = reader.accounts()?;
    ensure!(
        state["accounts"] == json!(accounts) && native["root"] == reader.root(),
        "Mixed V1 DB snapshots"
    );
    let events = super::v1_events::extract(&reader)?;
    let ledgers = state["native_protocols"]["receipt_ledgers"].take();
    ensure!(
        !ledgers.is_null(),
        "Missing freshly extracted receipt ledgers"
    );
    let balancer = super::v1_balancer::extract(&reader, &accounts, &ledgers, &events)?;
    let lending = super::v1_lending::extract(&reader, &accounts, &state, &ledgers)?;
    let evidence =
        super::v1_lending::verify_closeout_evidence(&reader, &accounts, &lending, &events)?;
    let closeout = super::v1_closeout::calculate(&lending, &accounts)?;
    let wrappers = super::v1_wrappers::extract(&reader, &accounts, &state, &balancer)?;
    state["native_protocols"]["receipt_ledgers"] = ledgers;
    state["native_protocols"]["balancer"] = balancer;
    state["native_protocols"]["lending_closeout"] = closeout;
    state["native_protocols"]["lending_evidence"] = evidence;
    state["native_protocols"]["wrapper_rights"] = wrappers;
    let allocation = reconstruct_base(&native, &state, &reader)?;
    super::v1_reports::calculate(&native, &state, &allocation)
}
