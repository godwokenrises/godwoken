use super::{
    layout::{Layout, Records},
    plan::{self, Plan, Tree},
};
use crate::{settlement::adapter::NativeDb, withdraw::state::RecoveryState};
use anyhow::{ensure, Context, Result};
use exit_withdraw_state_validator::{
    self as c,
    settlement::{self as s, records::*},
};
use gw_common::state::*;
use gw_types::{prelude::*, U256};
use std::collections::{BTreeMap, BTreeSet};
pub struct Ledger {
    pub recipient_scripts: BTreeMap<u32, crate::model::Script>,
    pub records: [Records; s::RECORD_KINDS],
    pub tree: Tree,
    pub metadata: Option<Metadata>,
    physical: Records,
    layout: Layout,
    deployment: c::Hash,
    asset_locations: BTreeMap<c::Hash, c::Hash>,
}
fn cv<T>(r: c::Result<T>) -> Result<T> {
    r.map_err(|e| anyhow::anyhow!("Settlement: {e:?}"))
}
impl Ledger {
    pub fn new(plan: &Plan) -> Result<Self> {
        plan.verify()?;
        let maps = plan.records()?;
        let mut records = std::array::from_fn(|_| BTreeMap::new());
        records[s::SOURCE_PLAN] = maps[0].clone();
        records[s::LOT_PLAN] = maps[1].clone();
        records[s::CLAIM_PLAN] = maps[2].clone();
        let limits = plan
            .limits
            .iter()
            .map(|v| plan::parse::<AssetLimit>(v))
            .collect::<Result<Vec<_>>>()?;
        Self::from_records(
            records,
            plan.recipient_scripts.clone(),
            crate::model::hash32(&plan.deployment)?,
            &limits,
        )
    }
    pub fn from_records(
        records: [Records; s::RECORD_KINDS],
        recipient_scripts: BTreeMap<u32, crate::model::Script>,
        deployment: c::Hash,
        limits: &[AssetLimit],
    ) -> Result<Self> {
        let maps = [
            records[s::SOURCE_PLAN].clone(),
            records[s::LOT_PLAN].clone(),
            records[s::CLAIM_PLAN].clone(),
        ];
        let layout = Layout::new(&maps)?;
        let asset_locations = limits
            .iter()
            .map(|l| {
                (
                    asset_key(deployment, l.asset),
                    s::physical::asset_state_key(l.asset),
                )
            })
            .collect();
        let mut out = Self {
            recipient_scripts,
            records,
            tree: Tree::default(),
            metadata: None,
            physical: Records::new(),
            layout,
            deployment,
            asset_locations,
        };
        out.rebuild_from_records()?;
        Ok(out)
    }
    pub fn state_root(&self) -> c::Hash {
        (*self.tree.root()).into()
    }
    pub fn physical_records(&self) -> &Records {
        &self.physical
    }
    fn location(&self, kind: usize, key: c::Hash) -> Result<c::Hash> {
        match kind {
            s::EPOCHS => {
                ensure!(key == epoch_key(self.deployment, 0), "Unknown epoch");
                Ok(s::physical::epoch_state_key(0))
            }
            s::ASSETS => self
                .asset_locations
                .get(&key)
                .copied()
                .context("Unknown asset state"),
            _ => self.layout.location(kind, key),
        }
    }
    /// Rebuild is used when importing an authenticated snapshot; normal replay updates touched pages only.
    pub fn rebuild_from_records(&mut self) -> Result<()> {
        let mut physical = self.layout.records.clone();
        for kind in [s::PAID, s::RESERVES, s::SOURCES] {
            for (key, value) in &self.records[kind] {
                let (location, raw) =
                    self.layout
                        .update_page(&physical, kind, *key, value.clone())?;
                physical.insert(location, raw);
            }
        }
        for kind in [s::EPOCHS, s::ASSETS] {
            for (key, value) in &self.records[kind] {
                physical.insert(self.location(kind, *key)?, value.clone());
            }
        }
        self.tree = plan::tree(&physical)?;
        self.physical = physical;
        Ok(())
    }
    pub fn get<T: Field>(&self, kind: usize, key: c::Hash) -> Result<T> {
        cv(decode(
            self.records[kind]
                .get(&key)
                .context("Missing ledger record")?,
        ))
    }
    pub fn source(&self, key: c::Hash) -> Result<Source> {
        self.get(s::SOURCE_PLAN, key)
    }
    pub fn paid(&self, d: c::Hash, claim: &Claim) -> Result<U256> {
        let k = claim_key(d, claim);
        Ok(if let Some(v) = self.records[s::PAID].get(&k) {
            cv(decode::<Paid>(v))?.amount
        } else {
            0.into()
        })
    }
    pub fn apply(&mut self, e: s::Evaluation) -> Result<()> {
        let mut changes = Records::new();
        for (kind, writes) in &e.tree_writes {
            ensure!(
                s::physical::mutable(*kind) || (self.metadata.is_none() && *kind == s::EPOCHS),
                "Immutable state write"
            );
            for (key, value) in writes {
                let location = self.location(*kind, *key)?;
                let raw = if matches!(*kind, s::EPOCHS | s::ASSETS) {
                    value.clone()
                } else {
                    let old = changes
                        .get(&location)
                        .or_else(|| self.physical.get(&location))
                        .context("Missing account page")?;
                    let mut page = cv(s::physical::unpack_page(old))?;
                    page.insert((*kind, *key), value.clone());
                    let raw = s::physical::pack_page(&page);
                    cv(s::physical::unpack_page(&raw))?;
                    raw
                };
                changes.insert(location, raw);
            }
        }
        self.tree.update_all(
            changes
                .iter()
                .map(|(key, value)| ((*key).into(), commitment(value).into()))
                .collect(),
        )?;
        ensure!(
            self.tree.root().as_slice() == e.metadata.state_root,
            "Ledger state root mismatch"
        );
        self.physical.extend(changes);
        for (kind, writes) in e.tree_writes {
            for (key, value) in writes {
                if value.is_empty() {
                    self.records[kind].remove(&key);
                } else {
                    self.records[kind].insert(key, value);
                }
            }
        }
        self.metadata = Some(e.metadata);
        Ok(())
    }
    pub fn build_action(
        &self,
        db: &dyn NativeDb,
        native: &RecoveryState<'_>,
        p: &s::SettlementProfile,
        profile_hash: c::Hash,
        op: s::Operation,
        epoch: u64,
        records: Vec<Vec<u8>>,
        action: c::Action,
    ) -> Result<s::SettlementAction> {
        self.build_action_with(
            p,
            profile_hash,
            op,
            epoch,
            records,
            action,
            db.protocol().native(),
            |keys| {
                let kv = keys
                    .iter()
                    .map(|k| Ok((*k, native.read(*k)?)))
                    .collect::<Result<_>>()?;
                let ks: Vec<_> = keys.iter().map(|k| (*k).into()).collect();
                Ok((kv, native.tree.merkle_proof(ks.clone())?.compile(ks)?))
            },
        )
    }
    /// Shared bounded action builder; storage and native-version authentication stay explicit.
    pub fn build_action_with(
        &self,
        p: &s::SettlementProfile,
        profile_hash: c::Hash,
        op: s::Operation,
        epoch: u64,
        records: Vec<Vec<u8>>,
        mut action: c::Action,
        version: c::native::NativeVersion,
        mut proof: impl FnMut(
            &BTreeSet<c::Hash>,
        )
            -> Result<(Vec<(c::Hash, c::Hash)>, gw_smt::smt::CompiledMerkleProof)>,
    ) -> Result<s::SettlementAction> {
        let d = p.deployment;
        let mut keys: BTreeMap<usize, BTreeSet<c::Hash>> = BTreeMap::new();
        let mut nk = BTreeSet::from([metadata_key(d, c::script_hash(&p.base.rollup))]);
        let mut need = |kind, key| {
            keys.entry(kind).or_default().insert(key);
        };
        let mut sources = Vec::new();
        match op {
            s::Operation::Begin => {
                need(s::EPOCHS, epoch_key(d, 0));
                for l in &p.limits {
                    need(s::ASSETS, asset_key(d, l.asset));
                }
            }
            s::Operation::Pay => {
                need(s::EPOCHS, epoch_key(d, epoch));
                for raw in &records {
                    let claim = cv(decode::<Payment>(raw))?.claim;
                    let ck = claim_key(d, &claim);
                    let src = self.source(claim.source)?;
                    need(s::SOURCES, claim.source);
                    need(s::ASSETS, asset_key(d, src.asset));
                    need(s::SOURCE_PLAN, claim.source);
                    need(s::LOT_PLAN, claim.lot);
                    need(s::RESERVES, claim.lot);
                    need(s::PAID, ck);
                    need(s::CLAIM_PLAN, ck);
                    if let Some(key) = version.supply_key(src.asset) {
                        nk.insert(key);
                    }
                    sources.push(src);
                }
            }
        }
        for r in &action.recipients {
            if c::control::is_descriptor(&r.script) {
                continue;
            }
            let sh = c::script_hash(&r.script);
            let address = if version == c::native::NativeVersion::V0 {
                sh[..20].try_into().unwrap()
            } else {
                r.address
            };
            version.identity_keys(r.id, sh, address, &mut nk);
        }
        for a in &action.assets {
            nk.insert(build_account_field_key(a.id, GW_ACCOUNT_SCRIPT_HASH_TYPE));
        }
        for src in sources {
            version.identity_keys(src.account, src.script_hash, src.address, &mut nk);
            nk.insert(version.balance_key(src.asset, src.address));
        }
        let (kv, native_proof) = proof(&nk)?;
        action.kv = kv;
        action.proof = native_proof;
        let mut physical_keys = BTreeSet::new();
        for (kind, logical_keys) in keys {
            for key in logical_keys {
                physical_keys.insert(self.location(kind, key)?);
            }
        }
        let leaves = physical_keys
            .iter()
            .map(|key| (*key, self.physical.get(key).cloned().unwrap_or_default()))
            .collect();
        let ks: Vec<_> = physical_keys.into_iter().map(Into::into).collect();
        let trees = vec![s::TreeProof {
            kind: s::physical::STATE,
            leaves,
            proof: self.tree.merkle_proof(ks.clone())?.compile(ks)?,
        }];
        let mut out = s::SettlementAction {
            op,
            epoch,
            before: self.metadata.as_ref().map(encode).unwrap_or_default(),
            after: vec![],
            records,
            trees,
            native: action,
        };
        let evaluated = cv(s::evaluate_version(p, profile_hash, &out, version))?;
        out.after = encode(&evaluated.metadata);
        Ok(out)
    }
    /// Deterministically select the next bounded administrative step; never predicts a confirmed state.
    pub fn next(
        &self,
        p: &s::SettlementProfile,
        batch: usize,
    ) -> Result<Option<(s::Operation, Vec<Vec<u8>>)>> {
        self.next_with_profile_hash(gw_hash::blake2b::hash(&s::codec::encode_profile(p)), batch)
    }
    /// V0 callers supply the hash of their entire versioned profile envelope.
    pub fn next_with_profile_hash(
        &self,
        profile_hash: c::Hash,
        batch: usize,
    ) -> Result<Option<(s::Operation, Vec<Vec<u8>>)>> {
        ensure!(
            batch > 0 && batch <= s::MAX_RECORDS,
            "Batch size must be 1..64"
        );
        let Some(m) = self.metadata else {
            return Ok(Some((s::Operation::Begin, vec![])));
        };
        ensure!(m.profile == profile_hash, "Profile mismatch");
        Ok(None)
    }
}
pub fn empty_action() -> c::Action {
    c::Action {
        recipients: vec![],
        assets: vec![],
        payouts: vec![],
        kv: vec![],
        proof: gw_smt::smt::CompiledMerkleProof(vec![]),
    }
}
pub fn apply_native(
    native: &mut RecoveryState<'_>,
    e: &s::Evaluation,
    root: c::Hash,
) -> Result<()> {
    for (k, v) in &e.native_writes {
        native.tree.update((*k).into(), (*v).into())?;
    }
    ensure!(
        native.root() == root,
        "Replayed native/metadata root mismatch"
    );
    Ok(())
}
