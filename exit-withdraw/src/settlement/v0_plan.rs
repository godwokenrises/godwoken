//! Deterministic v0 release export. Every beneficiary and source is authenticated to the DB root.
use super::{
    adapter::{NativeDb, Protocol},
    *,
};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{fs, path::Path};
#[derive(Deserialize)]
struct SourceRow {
    source_account_id: u32,
    asset_account_id: u32,
    raw_balance: String,
    allocated_raw: String,
    reserved_raw: String,
    reason: String,
}
#[derive(Deserialize)]
struct ClaimRow {
    beneficiary_account_id: u32,
    owner_eth_address: String,
    asset_account_id: u32,
    raw_amount: String,
    source_account_id: u32,
    route: String,
    status: String,
}
#[derive(Deserialize)]
struct RawRow {
    unregistered_address: String,
    native_asset_id: u32,
    raw_amount: String,
}
fn n(s: &str) -> Result<U256> {
    ensure!(
        !s.is_empty() && s.bytes().all(|v| v.is_ascii_digit()) && (s == "0" || !s.starts_with('0')),
        "Noncanonical integer"
    );
    U256::from_dec_str(s).map_err(|e| anyhow::anyhow!("{e:?}"))
}
pub fn export(
    db: &crate::v0::V0Db,
    dir: &Path,
    unmapped: &Path,
    successor: Option<&Path>,
    deployment: c::Hash,
    policy: c::Hash,
    bundle: &Bundle,
) -> Result<plan::Plan> {
    let (gs, cfg) = adapter::validate(bundle, Protocol::V0)?;
    ensure!(
        db.root() == db.tip["account_root"] && db.root() == gs.account_root,
        "v0 DB/L1 root mismatch"
    );
    db.anchor(&packed::GlobalState::from_slice(&bytes(
        &bundle.rollup_cell.output_data,
    )?)?)?;
    let mut checksums = BTreeMap::new();
    let mut load = |name: &str, path: &Path| -> Result<Vec<u8>> {
        let b = fs::read(path)?;
        checksums.insert(name.into(), hex::encode(Sha256::digest(&b)));
        Ok(b)
    };
    if db.tip["hash"] != gs.tip_hash {
        let proof: serde_json::Value = serde_json::from_slice(&load(
            "empty-successor.json",
            successor
                .context("Provide --empty-successor to authenticate the DB's one-block lag")?,
        )?)?;
        crate::v0::verify_empty_successor(
            &db.tip,
            &proof,
            &bytes(&bundle.rollup_cell.output_data)?,
        )?;
    }
    load(
        "account-assets.csv",
        &dir.parent()
            .context("Tables parent")?
            .join("reconciliation/account-assets.csv"),
    )?;
    let assets: Vec<serde_json::Value> = serde_json::from_slice(&load(
        "asset-coverage.json",
        &dir.join("asset-coverage.json"),
    )?)?;
    let mut limits = BTreeMap::new();
    for a in assets {
        let id = u32::try_from(a["asset_account_id"].as_u64().context("Asset ID")?)?;
        let cap = n(a["allocated_research_raw"].as_str().context("Ceiling")?)?;
        let custody = n(a["custodian_raw"].as_str().context("Custody")?)?;
        ensure!(
            a["all_sources_reconcile"] == true && cap <= custody,
            "Unbalanced asset table"
        );
        if custody.is_zero() {
            ensure!(cap.is_zero(), "Unbacked payable asset");
            continue;
        }
        let h = hash32(a["l1_type_hash"].as_str().context("Type hash")?)?;
        let (_, sc) = db.account(id)?;
        ensure!(
            sc.code_hash == hx(cfg.packed.l2_sudt_validator_script_type_hash().as_slice())
                && sc.hash_type == "type"
                && bytes(&sc.args)? == [hash32(crate::v0::ROLLUP)?.as_slice(), &h].concat(),
            "Asset identity mismatch"
        );
        ensure!(
            limits
                .insert(
                    id,
                    AssetLimit {
                        asset: id,
                        type_hash: h,
                        ceiling: cap
                    }
                )
                .is_none(),
            "Duplicate asset"
        );
    }
    let data = load("source-reserves.csv", &dir.join("source-reserves.csv"))?;
    let rows = csv::Reader::from_reader(data.as_slice())
        .deserialize::<SourceRow>()
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let mut sources = BTreeMap::new();
    let mut lots = BTreeMap::new();
    let mut lookup = BTreeMap::new();
    for row in rows {
        let Some(limit) = limits.get(&row.asset_account_id) else {
            ensure!(
                n(&row.allocated_raw)?.is_zero(),
                "Payable source outside custody catalogue"
            );
            continue;
        };
        let baseline = n(&row.raw_balance)?;
        let allocated = n(&row.allocated_raw)?;
        let reserved = n(&row.reserved_raw)?;
        ensure!(
            allocated.checked_add(reserved) == Some(baseline),
            "Source partition mismatch"
        );
        if baseline.is_zero() {
            continue;
        }
        let (sh, _) = db.account(row.source_account_id)?;
        let address: [u8; 20] = sh[..20].try_into()?;
        let v = Protocol::V0
            .native()
            .amount(
                db.raw(
                    Protocol::V0
                        .native()
                        .balance_key(row.asset_account_id, address),
                )?,
            )
            .map_err(|e| anyhow::anyhow!("{e:?}"))?;
        ensure!(
            v == baseline,
            "Source balance differs from authenticated state"
        );
        let source = Source {
            account: row.source_account_id,
            script_hash: sh,
            address,
            asset: row.asset_account_id,
            type_hash: limit.type_hash,
            baseline,
            evidence: hash(
                format!(
                    "v0:{}:{}:{}",
                    row.source_account_id, row.asset_account_id, row.raw_balance
                )
                .as_bytes(),
            ),
        };
        let key = source_key(deployment, &source);
        ensure!(
            sources.insert(key, source).is_none()
                && lookup
                    .insert((row.source_account_id, row.asset_account_id), key)
                    .is_none(),
            "Duplicate source"
        );
        for (kind, amount) in [(INITIAL, allocated), (PROTECTED, reserved)] {
            if !amount.is_zero() {
                // All non-payable rights remain protected in this release. Later policy needs a new reviewed release.
                let lot = Lot {
                    id: hash(&[key.as_slice(), &[kind]].concat()),
                    source: key,
                    kind,
                    amount,
                    evidence: hash(row.reason.as_bytes()),
                    reopen: hash(b"v0 published disposition; no automatic reopening"),
                };
                lots.insert(lot_key(deployment, &lot), lot);
            }
        }
    }
    let data = load("unmapped-balances.csv", unmapped)?;
    for row in csv::Reader::from_reader(data.as_slice()).deserialize::<RawRow>() {
        let row = row?;
        let Some(limit) = limits.get(&row.native_asset_id) else {
            continue;
        };
        let baseline = n(&row.raw_amount)?;
        if baseline.is_zero() {
            continue;
        }
        let address = eth_address(&row.unregistered_address)?;
        ensure!(
            address != [0; 20] && db.raw(c::native::short_key(address))? == [0; 32],
            "Raw destination is registered"
        );
        ensure!(
            Protocol::V0
                .native()
                .amount(
                    db.raw(
                        Protocol::V0
                            .native()
                            .balance_key(row.native_asset_id, address)
                    )?
                )
                .map_err(|e| anyhow::anyhow!("{e:?}"))?
                == baseline,
            "Unmapped balance mismatch"
        );
        let src = Source {
            account: c::native::RAW_DESTINATION,
            script_hash: [0; 32],
            address,
            asset: row.native_asset_id,
            type_hash: limit.type_hash,
            baseline,
            evidence: hash(row.unregistered_address.as_bytes()),
        };
        let key = source_key(deployment, &src);
        ensure!(sources.insert(key, src).is_none(), "Duplicate raw source");
        let lot = Lot {
            id: hash(&[key.as_slice(), b"raw protected"].concat()),
            source: key,
            kind: PROTECTED,
            amount: baseline,
            evidence: hash(b"unmapped native short destination"),
            reopen: hash(b"no authenticated ETH owner"),
        };
        lots.insert(lot_key(deployment, &lot), lot);
    }
    let data = load("claims.csv", &dir.join("claims.csv"))?;
    let mut grouped = BTreeMap::<(c::Hash, u32, [u8; 20]), (U256, Vec<String>)>::new();
    for row in csv::Reader::from_reader(data.as_slice()).deserialize::<ClaimRow>() {
        let row = row?;
        ensure!(
            row.status == "eligible_research_only_capacity_and_execution_separate",
            "Unexpected v0 claim disposition"
        );
        let source = *lookup
            .get(&(row.source_account_id, row.asset_account_id))
            .context("Claim has no backed source")?;
        let address = eth_address(&row.owner_eth_address)?;
        let v = grouped
            .entry((source, row.beneficiary_account_id, address))
            .or_default();
        v.0 =
            v.0.checked_add(n(&row.raw_amount)?)
                .context("Claim overflow")?;
        v.1.push(row.route);
    }
    let initial: BTreeMap<_, _> = lots
        .iter()
        .filter(|(_, l)| l.kind == INITIAL)
        .map(|(k, l)| (l.source, *k))
        .collect();
    let mut claims = BTreeMap::new();
    for ((source, id, address), (cap, mut routes)) in grouped {
        let (sh, sc) = db.account(id)?;
        ensure!(
            sc.code_hash == crate::v0::ETH
                && sc.hash_type == "type"
                && bytes(&sc.args)? == [hash32(crate::v0::ROLLUP)?.as_slice(), &address].concat(),
            "Claim recipient is not this ETH owner"
        );
        routes.sort();
        let claim = Claim {
            epoch: 0,
            source,
            lot: *initial.get(&source).context("No initial lot")?,
            recipient: id,
            script_hash: sh,
            address,
            cap,
            evidence: hash(&serde_json::to_vec(&routes)?),
        };
        claims.insert(claim_key(deployment, &claim), claim);
    }
    let mut p = plan::Plan {
        recipient_scripts: BTreeMap::new(),
        protocol: Protocol::V0,
        version: s::FORMAT_VERSION,
        deployment: hx(deployment),
        snapshot_root: db.root(),
        manifest: String::new(),
        policy: hx(policy),
        sources: sources.values().map(|s| hx(encode(s))).collect(),
        reserves: lots.values().map(|s| hx(encode(s))).collect(),
        claims: claims.values().map(|s| hx(encode(s))).collect(),
        limits: limits.values().map(|s| hx(encode(s))).collect(),
        state_root: String::new(),
        input_sha256: checksums,
    };
    let maps = p.records()?;
    p.state_root = plan::initial_root(&maps)?;
    p.manifest = p.compute_manifest()?;
    p.verify()?;
    Ok(p)
}
