use super::plan::{self, Plan, Tree};
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
    pub records: [BTreeMap<c::Hash, Vec<u8>>; 12],
    pub trees: [Tree; 12],
    pub metadata: Option<Metadata>,
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
        let mut trees = std::array::from_fn(|_| Tree::default());
        for i in 0..12 {
            trees[i] = plan::tree(&records[i])?;
        }
        Ok(Self {
            recipient_scripts: plan.recipient_scripts.clone(),
            records,
            trees,
            metadata: None,
        })
    }
    pub fn get<T: Field>(&self, kind: usize, key: c::Hash) -> Result<T> {
        cv(decode(
            self.records[kind]
                .get(&key)
                .context("Missing ledger record")?,
        ))
    }
    /// Immutable authenticated catalogue, including sources not yet paid on-chain.
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
        for (kind, writes) in e.tree_writes {
            for (k, v) in writes {
                self.trees[kind].update(k.into(), commitment(&v).into())?;
                if v.is_empty() {
                    self.records[kind].remove(&k);
                } else {
                    self.records[kind].insert(k, v);
                }
            }
        }
        if e.metadata.has_pending == 0 {
            self.records[s::REGISTERED].clear();
            self.trees[s::REGISTERED] = Tree::default();
        }
        for i in 0..8 {
            ensure!(
                self.trees[i].root().as_slice() == e.metadata.roots[i],
                "Ledger root mismatch for tree {i}"
            );
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
            |id| {
                let (_, s) = db.account(id)?;
                s.packed()
            },
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
        mut account: impl FnMut(u32) -> Result<gw_types::packed::Script>,
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
        let mut recipients = BTreeMap::new();
        let mut assets = BTreeSet::new();
        match op {
            s::Operation::Begin => {
                need(s::EPOCHS, epoch_key(d, 0));
                for l in &p.limits {
                    need(s::ASSETS, asset_key(d, l.asset));
                }
            }
            s::Operation::RegisterSources => {
                for raw in &records {
                    let src: Source = cv(decode(raw))?;
                    let k = source_key(d, &src);
                    need(s::SOURCES, k);
                    need(s::SOURCE_PLAN, k);
                    assets.insert(src.asset);
                    sources.push(src);
                }
            }
            s::Operation::RegisterReserves => {
                for raw in &records {
                    let l: Lot = cv(decode(raw))?;
                    let k = lot_key(d, &l);
                    need(s::SOURCES, l.source);
                    for t in [s::RESERVES, s::LOT_RECEIPTS, s::LOT_PLAN] {
                        need(t, k);
                    }
                }
            }
            s::Operation::FinalizeSources => {
                for raw in &records {
                    let k: c::Hash = cv(decode(raw))?;
                    need(s::SOURCES, k);
                    need(s::COMPLETED, k);
                }
            }
            s::Operation::SealBudgets => {}
            s::Operation::BeginEpoch => {
                need(s::EPOCHS, epoch_key(d, epoch));
            }
            s::Operation::Activate | s::Operation::Abort => {
                need(s::EPOCHS, epoch_key(d, epoch));
            }
            s::Operation::RegisterClaims | s::Operation::Unregister | s::Operation::Pay => {
                need(s::EPOCHS, epoch_key(d, epoch));
                for raw in &records {
                    let claim = if op == s::Operation::Pay {
                        cv(decode::<Payment>(raw))?.claim
                    } else {
                        cv(decode::<Claim>(raw))?
                    };
                    let ck = claim_key(d, &claim);
                    need(s::SOURCES, claim.source);
                    let src = self.source(claim.source)?;
                    need(s::ASSETS, asset_key(d, src.asset));
                    match op {
                        s::Operation::Pay => {
                            need(s::SOURCE_PLAN, claim.source);
                            need(s::LOT_PLAN, claim.lot);
                            need(s::RESERVES, claim.lot);
                            need(s::PAID, ck);
                            need(s::CLAIM_PLAN, ck);
                            sources.push(src);
                            assets.insert(src.asset);
                            if let Some(key) = version.supply_key(src.asset) {
                                nk.insert(key);
                            }
                            recipients.insert(claim.address, claim.recipient);
                        }
                        s::Operation::RegisterClaims => {
                            need(s::CLAIM_PLAN, ck);
                            need(s::REGISTERED, ck);
                            need(s::RESERVES, claim.lot);
                            if epoch > 0 {
                                need(s::ALLOWED_LOTS, claim.lot);
                            }
                            recipients.insert(claim.address, claim.recipient);
                        }
                        _ => {
                            need(s::REGISTERED, ck);
                            need(s::PAID, ck);
                            need(s::RESERVES, claim.lot);
                        }
                    }
                }
            }
        }
        // Payout shape already constructed by the cell builder. Registration derives its own EOA list.
        if op == s::Operation::RegisterClaims {
            for (address, id) in &recipients {
                let script = if let Some(s) = self.recipient_scripts.get(id) {
                    s.packed()?
                } else {
                    account(*id)?
                };
                action.recipients.push(c::Recipient {
                    address: *address,
                    id: *id,
                    script,
                });
            }
        }
        if op == s::Operation::RegisterSources {
            for id in assets {
                let script = account(id)?;
                action.assets.push(c::Asset { id, script });
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
        let trees = keys
            .into_iter()
            .map(|(kind, keys)| {
                let leaves = keys
                    .iter()
                    .map(|k| (*k, self.records[kind].get(k).cloned().unwrap_or_default()))
                    .collect();
                let ks: Vec<_> = keys.into_iter().map(Into::into).collect();
                Ok(s::TreeProof {
                    kind,
                    leaves,
                    proof: self.trees[kind].merkle_proof(ks.clone())?.compile(ks)?,
                })
            })
            .collect::<Result<_>>()?;
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
            "Batch size must be 1..32"
        );
        let Some(m) = self.metadata else {
            return Ok(Some((s::Operation::Begin, vec![])));
        };
        ensure!(
            m.phase == 2 && m.has_pending == 0,
            "Expected active root-only release"
        );
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
