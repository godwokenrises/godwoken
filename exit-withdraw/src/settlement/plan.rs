use crate::{db::Database, model::*};
use anyhow::{ensure, Context, Result};
use exit_withdraw_state_validator::{
    self as c,
    settlement::{self as s, records::*},
};
use gw_common::state::State;
use gw_smt::smt::{default_store::DefaultStore, SMT, SMTH256};
use gw_types::{prelude::*, U256};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::Path};
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    #[serde(default, skip_serializing_if = "super::adapter::Protocol::is_v1")]
    pub protocol: super::adapter::Protocol,
    pub version: u32,
    pub deployment: String,
    pub snapshot_root: String,
    pub manifest: String,
    pub policy: String,
    pub sources: Vec<String>,
    pub reserves: Vec<String>,
    pub claims: Vec<String>,
    pub limits: Vec<String>,
    pub state_root: String,
    pub input_sha256: BTreeMap<String, String>,
    /// Root-bound reviewed controller descriptors; empty for EOA-only plans.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub recipient_scripts: BTreeMap<u32, Script>,
}
pub type Tree = SMT<DefaultStore<SMTH256>>;
pub fn tree(records: &BTreeMap<c::Hash, Vec<u8>>) -> Result<Tree> {
    let mut t = Tree::default();
    t.update_all(
        records
            .iter()
            .map(|(k, v)| ((*k).into(), commitment(v).into()))
            .collect(),
    )?;
    Ok(t)
}
pub fn root(records: &BTreeMap<c::Hash, Vec<u8>>) -> Result<String> {
    Ok(hx(tree(records)?.root().as_slice()))
}
pub fn initial_root(maps: &[BTreeMap<c::Hash, Vec<u8>>; 3]) -> Result<String> {
    let layout = super::layout::Layout::new(maps)?;
    root(&layout.records)
}
pub fn parse<T: Field>(s: &str) -> Result<T> {
    decode(&bytes(s)?).map_err(|e| anyhow::anyhow!("Record: {e:?}"))
}
impl Plan {
    pub fn records(&self) -> Result<[BTreeMap<c::Hash, Vec<u8>>; 3]> {
        let d = hash32(&self.deployment)?;
        let mut maps = std::array::from_fn(|_| BTreeMap::new());
        for v in &self.sources {
            let x: Source = parse(v)?;
            ensure!(
                maps[0].insert(source_key(d, &x), bytes(v)?).is_none(),
                "Duplicate source"
            );
        }
        for v in &self.reserves {
            let x: Lot = parse(v)?;
            ensure!(
                maps[1].insert(lot_key(d, &x), bytes(v)?).is_none(),
                "Duplicate reserve"
            );
        }
        for v in &self.claims {
            let x: Claim = parse(v)?;
            ensure!(
                maps[2].insert(claim_key(d, &x), bytes(v)?).is_none(),
                "Duplicate claim"
            );
        }
        Ok(maps)
    }
    pub fn verify(&self) -> Result<serde_json::Value> {
        ensure!(self.version == s::FORMAT_VERSION, "Unknown plan version");
        let d = hash32(&self.deployment)?;
        let maps = self.records()?;
        for (id, script) in &self.recipient_scripts {
            c::control::validate(&script.packed()?, hash32(self.protocol.rollup())?, *id)
                .map_err(|e| anyhow::anyhow!("Control descriptor: {e:?}"))?;
            ensure!(
                self.claims
                    .iter()
                    .any(|raw| parse::<Claim>(raw).is_ok_and(|c| c.recipient == *id)),
                "Unused controller descriptor"
            );
        }
        ensure!(
            self.state_root == initial_root(&maps)?,
            "Plan state root mismatch"
        );
        let mut lot_totals = BTreeMap::<c::Hash, U256>::new();
        let mut claim_totals = BTreeMap::<c::Hash, U256>::new();
        let mut asset_totals = BTreeMap::<u32, U256>::new();
        for raw in maps[1].values() {
            let l: Lot = decode(raw).map_err(|e| anyhow::anyhow!("{e:?}"))?;
            ensure!(
                maps[0].contains_key(&l.source) && l.kind <= PROTECTED && !l.amount.is_zero(),
                "Invalid lot"
            );
            let src: Source = decode(&maps[0][&l.source]).map_err(|e| anyhow::anyhow!("{e:?}"))?;
            ensure!(
                !self.protocol.native().protected(&src) || l.kind == PROTECTED,
                "Zero-address custody must remain protected"
            );
            add(&mut lot_totals, l.source, l.amount)?;
        }
        for raw in maps[2].values() {
            let claim: Claim = decode(raw).map_err(|e| anyhow::anyhow!("{e:?}"))?;
            ensure!(
                claim.epoch == 0 && !claim.cap.is_zero(),
                "Invalid initial claim"
            );
            let l: Lot = decode(maps[1].get(&claim.lot).context("Missing claim lot")?)
                .map_err(|e| anyhow::anyhow!("{e:?}"))?;
            ensure!(
                l.kind == INITIAL && l.source == claim.source,
                "Claim spends a retained lot"
            );
            let src: Source = decode(maps[0].get(&claim.source).context("Missing source")?)
                .map_err(|e| anyhow::anyhow!("{e:?}"))?;
            add(&mut claim_totals, claim.lot, claim.cap)?;
            add(&mut asset_totals, src.asset, claim.cap)?;
        }
        for (k, raw) in &maps[0] {
            let src: Source = decode(raw).map_err(|e| anyhow::anyhow!("{e:?}"))?;
            ensure!(
                lot_totals.get(k) == Some(&src.baseline),
                "Source partition does not equal baseline"
            );
        }
        for (k, raw) in &maps[1] {
            let l: Lot = decode(raw).map_err(|e| anyhow::anyhow!("{e:?}"))?;
            if l.kind == INITIAL {
                ensure!(
                    claim_totals.get(k) == Some(&l.amount),
                    "Incomplete initial allocation"
                );
            }
        }
        let mut catalogue = BTreeMap::new();
        let mut last = None;
        for raw in &self.limits {
            let l: AssetLimit = parse(raw)?;
            ensure!(
                last.is_none_or(|v| v < l.asset),
                "Unsorted/duplicate asset limit"
            );
            last = Some(l.asset);
            catalogue.insert(l.asset, l.type_hash);
            ensure!(
                asset_totals.remove(&l.asset).unwrap_or_default() <= l.ceiling,
                "Asset funding ceiling exceeded"
            );
        }
        ensure!(asset_totals.is_empty(), "Missing asset funding limit");
        ensure!(
            d != [0; 32]
                && hash32(&self.policy)? != [0; 32]
                && !maps[0].is_empty()
                && !maps[2].is_empty(),
            "Empty deployment/policy/plan"
        );
        ensure!(
            catalogue.get(&1) == Some(&[0; 32]) && catalogue.len() <= c::MAX_ASSETS,
            "Invalid native asset catalogue"
        );
        ensure!(
            catalogue
                .values()
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                == catalogue.len(),
            "Duplicate asset type hash"
        );
        for raw in maps[0].values() {
            let src: Source = decode(raw).map_err(|e| anyhow::anyhow!("{e:?}"))?;
            ensure!(
                !src.baseline.is_zero()
                    && (src.script_hash != [0; 32]
                        || (self.protocol == super::adapter::Protocol::V0
                            && src.account == c::native::RAW_DESTINATION
                            && src.address != [0; 20]))
                    && catalogue.get(&src.asset) == Some(&src.type_hash),
                "Invalid source identity/catalogue"
            );
        }
        for raw in maps[1].values() {
            let l: Lot = decode(raw).map_err(|e| anyhow::anyhow!("{e:?}"))?;
            ensure!(l.id != [0; 32], "Zero lot ID");
        }
        for raw in maps[2].values() {
            let c: Claim = decode(raw).map_err(|e| anyhow::anyhow!("{e:?}"))?;
            if let Some(script) = self.recipient_scripts.get(&c.recipient) {
                let script = script.packed()?;
                ensure!(
                    c.script_hash == c::script_hash(&script)
                        && c.address == c::control::identity(&script),
                    "Control claim identity mismatch"
                );
            }
            ensure!(
                c.address != [0; 20] && c.script_hash != [0; 32],
                "Invalid beneficiary identity"
            );
        }
        ensure!(
            self.manifest == self.compute_manifest()?,
            "Manifest mismatch"
        );
        Ok(
            serde_json::json!({"version":s::FORMAT_VERSION,"status":"verified_plan_not_deployed","deployment":hx(d),"sources":maps[0].len(),"reserves":maps[1].len(),"claims":maps[2].len(),"state_root":self.state_root}),
        )
    }
    pub(crate) fn compute_manifest(&self) -> Result<String> {
        let mut copy = self.clone();
        copy.manifest.clear();
        Ok(hx(hash(&serde_json::to_vec(&copy)?)))
    }
    pub fn epoch(&self) -> Result<Epoch> {
        Ok(Epoch {
            id: 0,
            claim_count: self.claims.len() as u64,
            manifest: hash32(&self.manifest)?,
            policy: hash32(&self.policy)?,
            status: PENDING,
        })
    }
    pub fn bind(&self, p: &s::SettlementProfile) -> Result<()> {
        self.verify()?;
        ensure!(
            hash32(&self.deployment)? == p.deployment
                && hash32(&self.snapshot_root)?.as_slice()
                    == p.base.bootstrap_state.account().merkle_root().as_slice()
                && hash32(&self.manifest)? == p.manifest
                && hash32(&self.state_root)? == p.state_root
                && self.sources.len() as u64 == p.source_count
                && self.reserves.len() as u64 == p.reserve_count
                && self.epoch()? == p.initial_epoch,
            "Plan/profile mismatch"
        );
        ensure!(
            self.limits
                .iter()
                .map(|x| parse::<AssetLimit>(x))
                .collect::<Result<Vec<_>>>()?
                == p.limits,
            "Funding limits differ"
        );
        Ok(())
    }
}
fn add<K: Ord>(m: &mut BTreeMap<K, U256>, k: K, n: U256) -> Result<()> {
    let v = m.entry(k).or_default();
    *v = v.checked_add(n).context("Integer overflow")?;
    Ok(())
}
#[derive(Deserialize)]
struct Allocation {
    source_account: u32,
    asset_id: u32,
    beneficiary: String,
    beneficiary_account_id: Option<u32>,
    quantity_raw: String,
    status: String,
    path: String,
}
#[derive(Deserialize)]
struct Check {
    source_account: u32,
    asset_id: u32,
    input_raw: String,
    allocated_and_reserved_raw: String,
    pass: bool,
}
#[derive(Deserialize)]
struct AssetRow {
    asset_id: u32,
    type_hash: String,
    eligible_before_capacity_raw: String,
    custodian_covers_eligible_before_capacity: bool,
    l2_covers_eligible_before_capacity: bool,
}
fn integer(s: &str) -> Result<U256> {
    ensure!(
        !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) && (s == "0" || !s.starts_with('0')),
        "Noncanonical raw integer"
    );
    U256::from_dec_str(s).map_err(|e| anyhow::anyhow!("{e:?}"))
}
/// All current statuses require an explicit disposition. Unknown statuses never become payable.
fn kind(status: &str) -> Result<u8> {
    Ok(match status {
        "CALCULATED_EOA" => INITIAL,
        "HOLD_LENDING_CLOSEOUT"
        | "DEFER_BPT_LOW_TOTAL_VALUE"
        | "DEFER_LP12697_LOW_VALUE"
        | "DEFER_CREATOR_ROYALTIES"
        | "HOLD_MANUAL_TRANSFER_CLAIM"
        | "RESERVED_UNREGISTERED" => DEFERRED,
        "RESERVED_PROTOCOL_OR_ROUNDING" | "RESERVED_LOCKED" | "RESERVED_ZERO_SUPPLY" => PROTECTED,
        "IGNORE_KOI"
        | "IGNORE_KOI_UNRESOLVED"
        | "IGNORE_RESIDUAL_BELOW_100"
        | "IGNORE_CONTROL"
        | "IGNORE_RESEARCH_SCOPE"
        | "HOLD_PROJECT_SALE_CUSTODY"
        | "IGNORE_OLD_AAVE_POOL"
        | "HOLD_NEW_CONTROL"
        | "EXCLUDE_PENDING_BONUS_RESERVE"
        | "EXCLUDE_IDLE_YOK_BONUS"
        | "EXCLUDE_UNCLAIMED_CKB_REWARD" => EXCLUDED,
        _ => anyhow::bail!("Unmapped disposition {status}"),
    })
}
pub fn export(
    db: &Database,
    dir: &Path,
    deployment: c::Hash,
    policy: c::Hash,
    bundle: &Bundle,
) -> Result<Plan> {
    use sha2::{Digest, Sha256};
    ensure!(
        db.root() == db.tip.account_root,
        "Snapshot storage root differs from tip"
    );
    let (global, cfg) = bundle.validate()?;
    ensure!(
        global.account_root == db.root()
            && global.tip_hash == db.tip.hash
            && global.account_count == db.tip.account_count,
        "Snapshot DB and L1 anchor differ"
    );
    let mut input_sha256 = BTreeMap::new();
    let mut load = |name: &str| -> Result<Vec<u8>> {
        let b = std::fs::read(dir.join(name))?;
        input_sha256.insert(name.into(), hex::encode(Sha256::digest(&b)));
        Ok(b)
    };
    let summary: serde_json::Value = serde_json::from_slice(&load("summary.json")?)?;
    ensure!(
        summary["snapshot_root"].as_str() == Some(db.root().as_str()),
        "Published table snapshot root differs from DB"
    );
    let allocations: Vec<Allocation> = serde_json::from_slice(&load("source-allocations.json")?)?;
    let checks: Vec<Check> = serde_json::from_slice(&load("source-checks.json")?)?;
    let assets: Vec<AssetRow> = serde_json::from_slice(&load("assets.json")?)?;
    let mut sources = BTreeMap::new();
    let mut src_keys = BTreeMap::new();
    let mut limits = Vec::new();
    let mut types = BTreeMap::new();
    for asset in assets {
        ensure!(
            asset.custodian_covers_eligible_before_capacity
                && asset.l2_covers_eligible_before_capacity,
            "Uncovered asset {}",
            asset.asset_id
        );
        let h = hash32(&asset.type_hash)?;
        ensure!(
            types.insert(asset.asset_id, h).is_none(),
            "Duplicate asset row"
        );
        let (_, script) = db.account(asset.asset_id)?;
        let args = bytes(&script.args)?;
        ensure!(
            script.code_hash == cfg.hashes["l2_sudt"]
                && script.hash_type == "type"
                && args.len() == 64
                && args[..32] == db.chain_id
                && args[32..] == h,
            "Native asset identity/type hash differs from table"
        );
        limits.push(AssetLimit {
            asset: asset.asset_id,
            type_hash: h,
            ceiling: integer(&asset.eligible_before_capacity_raw)?,
        });
    }
    limits.sort_by_key(|l| l.asset);
    for check in checks {
        ensure!(
            check.pass && check.input_raw == check.allocated_and_reserved_raw,
            "Unbalanced source table"
        );
        let baseline = integer(&check.input_raw)?;
        if baseline.is_zero() {
            continue;
        }
        let (sh, _) = db.account(check.source_account)?;
        let reg = db
            .registry(&sh)?
            .context("Source has no native ETH registry address")?;
        let address = reg.address.as_slice().try_into()?;
        ensure!(
            db.lookup_eth(&address)?["account_id"].as_u64() == Some(check.source_account as u64),
            "Source account registry mapping differs"
        );
        ensure!(
            db.get_sudt_balance(check.asset_id, &reg)
                .map_err(|e| anyhow::anyhow!("{e:?}"))?
                == baseline,
            "Source balance differs from authenticated DB"
        );
        let src = Source {
            account: check.source_account,
            script_hash: sh,
            address,
            asset: check.asset_id,
            type_hash: *types.get(&check.asset_id).context("Unknown asset")?,
            baseline,
            evidence: hash(
                format!(
                    "source:{}:{}:{}",
                    check.source_account, check.asset_id, check.input_raw
                )
                .as_bytes(),
            ),
        };
        let key = source_key(deployment, &src);
        ensure!(
            src_keys.insert((src.account, src.asset), key).is_none(),
            "Duplicate source row"
        );
        sources.insert(key, src);
    }
    let mut groups = BTreeMap::<(c::Hash, String), Vec<Allocation>>::new();
    for a in allocations {
        let n = integer(&a.quantity_raw)?;
        if n.is_zero() {
            continue;
        }
        kind(&a.status)?;
        let key = *src_keys
            .get(&(a.source_account, a.asset_id))
            .context("Allocation without source")?;
        groups.entry((key, a.status.clone())).or_default().push(a);
    }
    let mut reserves = BTreeMap::new();
    let mut claims = BTreeMap::new();
    for ((source, status), rows) in groups {
        let mut amount = U256::zero();
        let mut provenance = Vec::new();
        let mut users = BTreeMap::<(u32, [u8; 20]), (U256, Vec<String>)>::new();
        for r in rows {
            let n = integer(&r.quantity_raw)?;
            amount = amount.checked_add(n).context("Overflow")?;
            provenance.push(format!("{}:{}:{}", r.beneficiary, r.quantity_raw, r.path));
            if kind(&status)? == INITIAL {
                let id = r
                    .beneficiary_account_id
                    .context("Eligible beneficiary lacks account ID")?;
                let addr = eth_address(&r.beneficiary)?;
                let v = users.entry((id, addr)).or_default();
                v.0 = v.0.checked_add(n).context("Overflow")?;
                v.1.push(r.path);
            }
        }
        provenance.sort();
        let lot = Lot {
            id: hash(&[source.as_slice(), status.as_bytes()].concat()),
            source,
            kind: kind(&status)?,
            amount,
            evidence: hash(&serde_json::to_vec(&provenance)?),
            reopen: hash(status.as_bytes()),
        };
        let lk = lot_key(deployment, &lot);
        reserves.insert(lk, lot);
        for ((id, address), (cap, mut paths)) in users {
            paths.sort();
            let (sh, script) = db.account(id)?;
            let reg = db.registry(&sh)?.context("Recipient lacks registry")?;
            ensure!(reg.address == address, "Recipient registry mismatch");
            let script = script.packed()?;
            ensure!(
                u8::from(script.hash_type()) == 1
                    && cfg
                        .packed
                        .allowed_eoa_type_hashes()
                        .into_iter()
                        .any(|a| u8::from(a.type_()) == 1
                            && a.hash().as_slice() == script.code_hash().as_slice()),
                "Eligible beneficiary is not an allowed ETH EOA"
            );
            ensure!(
                db.lookup_eth(&address)?["account_id"].as_u64() == Some(id as u64),
                "Beneficiary account registry mapping differs"
            );
            let args = script.args().raw_data();
            ensure!(
                args.len() == 52 && args[..32] == db.chain_id && args[32..] == address,
                "Eligible recipient is not a simple ETH account"
            );
            let claim = Claim {
                epoch: 0,
                source,
                lot: lk,
                recipient: id,
                script_hash: sh,
                address,
                cap,
                evidence: hash(&serde_json::to_vec(&paths)?),
            };
            ensure!(
                claims
                    .insert(claim_key(deployment, &claim), claim)
                    .is_none(),
                "Duplicate claim"
            );
        }
    }
    let mut p = Plan {
        recipient_scripts: BTreeMap::new(),
        protocol: super::adapter::Protocol::V1,
        version: s::FORMAT_VERSION,
        deployment: hx(deployment),
        snapshot_root: db.root(),
        manifest: String::new(),
        policy: hx(policy),
        sources: sources.values().map(|v| hx(encode(v))).collect(),
        reserves: reserves.values().map(|v| hx(encode(v))).collect(),
        claims: claims.values().map(|v| hx(encode(v))).collect(),
        limits: limits.iter().map(|v| hx(encode(v))).collect(),
        state_root: String::new(),
        input_sha256,
    };
    let maps = p.records()?;
    p.state_root = initial_root(&maps)?;
    p.manifest = p.compute_manifest()?;
    p.verify()?;
    Ok(p)
}
