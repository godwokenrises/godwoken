//! Settlement-only consensus. All writes are derived from typed, authenticated records.
pub mod codec;
pub mod records;
use crate::native::NativeVersion;
use crate::{
    verify::{require, verify_transition, SettlementTransition},
    *,
};
use alloc::{
    collections::{BTreeMap, BTreeSet},
    vec::Vec,
};
use gw_smt::smt::{Blake2bHasher, CompiledMerkleProof};
use gw_types::{packed, prelude::*, U256};
use records::*;

pub const EPOCHS: usize = 0;
pub const PAID: usize = 1;
pub const SOURCES: usize = 2;
pub const RESERVES: usize = 3;
pub const ASSETS: usize = 4;
pub const COMPLETED: usize = 5;
pub const LOT_RECEIPTS: usize = 6;
pub const REGISTERED: usize = 7;
pub const SOURCE_PLAN: usize = 8;
pub const LOT_PLAN: usize = 9;
pub const CLAIM_PLAN: usize = 10;
pub const ALLOWED_LOTS: usize = 11;
pub const MAX_RECORDS: usize = 32;
pub const MAX_TREE_KEYS: usize = 256;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Operation {
    Begin = 0,
    RegisterSources = 1,
    RegisterReserves = 2,
    FinalizeSources = 3,
    SealBudgets = 4,
    RegisterClaims = 5,
    Activate = 6,
    Pay = 7,
    BeginEpoch = 8,
    Unregister = 9,
    Abort = 10,
}
impl Operation {
    pub fn from_u32(n: u32) -> Result<Self> {
        Ok(match n {
            0 => Self::Begin,
            1 => Self::RegisterSources,
            2 => Self::RegisterReserves,
            3 => Self::FinalizeSources,
            4 => Self::SealBudgets,
            5 => Self::RegisterClaims,
            6 => Self::Activate,
            7 => Self::Pay,
            8 => Self::BeginEpoch,
            9 => Self::Unregister,
            10 => Self::Abort,
            _ => return Err(Error::Encoding),
        })
    }
}
#[derive(Clone, Debug)]
pub struct SettlementProfile {
    pub base: Profile,
    pub deployment: Hash,
    pub manifest: Hash,
    pub source_root: Hash,
    pub source_count: u64,
    pub reserve_root: Hash,
    pub reserve_count: u64,
    pub initial_epoch: Epoch,
    pub limits: Vec<AssetLimit>,
}
#[derive(Clone, Debug)]
pub struct TreeProof {
    pub kind: usize,
    pub leaves: Vec<(Hash, Vec<u8>)>,
    pub proof: CompiledMerkleProof,
}
#[derive(Clone, Debug)]
pub struct SettlementAction {
    pub op: Operation,
    pub epoch: u64,
    pub before: Vec<u8>,
    pub after: Vec<u8>,
    pub records: Vec<Vec<u8>>,
    pub trees: Vec<TreeProof>,
    pub native: Action,
}
#[derive(Debug)]
pub struct Evaluation {
    pub metadata: Metadata,
    pub tree_writes: BTreeMap<usize, BTreeMap<Hash, Vec<u8>>>,
    pub native_writes: BTreeMap<Hash, Hash>,
    pub sources: BTreeMap<(u32, Eth), U256>,
    pub payments: BTreeMap<(u32, u32), U256>,
    pub reads: BTreeSet<Hash>,
}
struct Trees<'a> {
    proofs: &'a [TreeProof],
    roots: [Hash; 12],
    used: BTreeMap<usize, BTreeSet<Hash>>,
    writes: BTreeMap<usize, BTreeMap<Hash, Vec<u8>>>,
}
impl<'a> Trees<'a> {
    fn new(p: &'a [TreeProof], m: &Metadata, profile: &SettlementProfile) -> Result<Self> {
        require(
            p.len() <= 12 && p.windows(2).all(|w| w[0].kind < w[1].kind),
            Error::Encoding,
        )?;
        let mut total = 0;
        for t in p {
            require(
                t.kind < 12
                    && !t.leaves.is_empty()
                    && t.leaves.len() <= MAX_TREE_KEYS
                    && t.leaves.windows(2).all(|w| w[0].0 < w[1].0),
                Error::Keys,
            )?;
            require(
                t.proof.0.len() <= MAX_PROOF
                    && t.leaves.iter().all(|(_, v)| {
                        v.len() <= 1024 && (v.is_empty() || commitment(v) != [0; 32])
                    }),
                Error::Limit,
            )?;
            total += t.leaves.len();
        }
        require(total <= MAX_KEYS, Error::Limit)?;
        let mut roots = [[0; 32]; 12];
        roots[..8].copy_from_slice(&m.roots);
        roots[SOURCE_PLAN] = profile.source_root;
        roots[LOT_PLAN] = profile.reserve_root;
        Ok(Self {
            proofs: p,
            roots,
            used: BTreeMap::new(),
            writes: BTreeMap::new(),
        })
    }
    fn original(&mut self, t: usize, k: Hash) -> Result<&[u8]> {
        self.used.entry(t).or_default().insert(k);
        let p = self
            .proofs
            .iter()
            .find(|p| p.kind == t)
            .ok_or(Error::Proof)?;
        let i = p
            .leaves
            .binary_search_by_key(&k, |v| v.0)
            .map_err(|_| Error::Keys)?;
        Ok(&p.leaves[i].1)
    }
    fn get<T: Field>(&mut self, t: usize, k: Hash) -> Result<T> {
        self.original(t, k)?;
        if let Some(v) = self.writes.get(&t).and_then(|w| w.get(&k)) {
            decode(v)
        } else {
            decode(self.original(t, k)?)
        }
    }
    fn absent(&mut self, t: usize, k: Hash) -> Result<()> {
        self.original(t, k)?;
        let b = if let Some(v) = self.writes.get(&t).and_then(|w| w.get(&k)) {
            v.as_slice()
        } else {
            self.original(t, k)?
        };
        require(b.is_empty(), Error::State)
    }
    fn set<T: Field>(&mut self, t: usize, k: Hash, v: &T) -> Result<()> {
        self.original(t, k)?;
        self.writes.entry(t).or_default().insert(k, encode(v));
        Ok(())
    }
    fn finish(&self, m: &mut Metadata) -> Result<()> {
        require(self.used.len() == self.proofs.len(), Error::Keys)?;
        for p in self.proofs {
            require(
                self.used
                    .get(&p.kind)
                    .is_some_and(|u| u.len() == p.leaves.len()),
                Error::Keys,
            )?;
            let old = p
                .leaves
                .iter()
                .map(|(k, v)| ((*k).into(), commitment(v).into()))
                .collect();
            let r: Hash = p
                .proof
                .compute_root::<Blake2bHasher>(old)
                .map_err(|_| Error::Proof)?
                .into();
            require(r == self.roots[p.kind], Error::Proof)?;
            if let Some(w) = self.writes.get(&p.kind) {
                require(p.kind < 8, Error::State)?;
                let new = p
                    .leaves
                    .iter()
                    .map(|(k, v)| ((*k).into(), commitment(w.get(k).unwrap_or(v)).into()))
                    .collect();
                m.roots[p.kind] = p
                    .proof
                    .compute_root::<Blake2bHasher>(new)
                    .map_err(|_| Error::Proof)?
                    .into();
            }
        }
        Ok(())
    }
}
fn plus(a: U256, b: U256) -> Result<U256> {
    a.checked_add(b).ok_or(Error::Overflow)
}
fn minus(a: U256, b: U256) -> Result<U256> {
    a.checked_sub(b).ok_or(Error::Balance)
}
fn increment(a: u64) -> Result<u64> {
    a.checked_add(1).ok_or(Error::Overflow)
}
fn read_native(a: &Action, read: &mut BTreeSet<Hash>, key: Hash) -> Result<Hash> {
    read.insert(key);
    a.kv.binary_search_by_key(&key, |v| v.0)
        .map(|i| a.kv[i].1)
        .map_err(|_| Error::Keys)
}
fn source_identity(
    p: &SettlementProfile,
    a: &Action,
    s: &records::Source,
    reads: &mut BTreeSet<Hash>,
    version: NativeVersion,
) -> Result<()> {
    require(
        p.base.catalogue.contains(&(s.asset, s.type_hash)),
        Error::Asset,
    )?;
    version.identity(
        s.account,
        s.script_hash,
        s.address,
        p.base.bootstrap_state.account().count().unpack(),
        |k| read_native(a, reads, k),
    )
}

fn recipient(a: &Action, c: &Claim, used: &mut BTreeSet<u32>) -> Result<u32> {
    let n = a
        .recipients
        .binary_search_by_key(&c.address, |r| r.address)
        .map_err(|_| Error::Recipient)?;
    let r = &a.recipients[n];
    require(
        r.id == c.recipient && script_hash(&r.script) == c.script_hash,
        Error::Recipient,
    )?;
    used.insert(n as u32);
    Ok(n as u32)
}
fn check_profile(p: &SettlementProfile) -> Result<()> {
    require(
        p.deployment != [0; 32]
            && p.manifest != [0; 32]
            && p.source_root != [0; 32]
            && p.reserve_root != [0; 32]
            && p.source_count > 0
            && p.reserve_count > 0,
        Error::Profile,
    )?;
    let e = &p.initial_epoch;
    require(
        e.id == 0
            && e.status == PENDING
            && e.claims_root != [0; 32]
            && e.claim_count > 0
            && e.parent == [0; 32]
            && e.allowed_lots_root == [0; 32]
            && e.manifest != [0; 32]
            && e.policy != [0; 32],
        Error::Profile,
    )?;
    require(p.limits.len() == p.base.catalogue.len(), Error::Profile)?;
    for (l, (id, h)) in p.limits.iter().zip(&p.base.catalogue) {
        require(l.asset == *id && l.type_hash == *h, Error::Profile)?;
    }
    Ok(())
}
/// Derive checked logical updates. The caller must also authenticate the complete native SMT
/// transition and all transaction cells via `verify`; this function alone is not authorization.
pub fn evaluate(
    p: &SettlementProfile,
    profile_hash: Hash,
    a: &SettlementAction,
) -> Result<Evaluation> {
    evaluate_version(p, profile_hash, a, NativeVersion::V1)
}
pub fn evaluate_version(
    p: &SettlementProfile,
    profile_hash: Hash,
    a: &SettlementAction,
    version: NativeVersion,
) -> Result<Evaluation> {
    check_profile(p)?;
    // This release is immutable: one root commitment followed only by proven payments.
    require(
        matches!(a.op, Operation::Begin | Operation::Pay) && a.epoch == 0,
        Error::State,
    )?;
    require(
        a.records.len() <= MAX_RECORDS && a.records.iter().all(|r| r.len() <= 1024),
        Error::Limit,
    )?;
    let mk = metadata_key(p.deployment, script_hash(&p.base.rollup));
    let mut reads = BTreeSet::new();
    let old_native_meta = read_native(&a.native, &mut reads, mk)?;
    let mut m = if a.op == Operation::Begin {
        require(
            a.before.is_empty()
                && old_native_meta == [0; 32]
                && a.records.is_empty()
                && a.epoch == 0,
            Error::State,
        )?;
        Metadata {
            deployment: p.deployment,
            profile: profile_hash,
            snapshot: p.base.bootstrap_state.account().merkle_root().unpack(),
            sequence: 0,
            phase: 2,
            next_epoch: 1,
            pending_epoch: 0,
            has_pending: 0,
            roots: [[0; 32]; 8],
            source_count: 0,
            lot_count: 0,
            finalized_count: 0,
            registered_count: 0,
            initial_lots_left: 0,
        }
    } else {
        require(
            !a.before.is_empty()
                && commitment(&a.before) == old_native_meta
                && old_native_meta != [0; 32],
            Error::State,
        )?;
        let m: Metadata = decode(&a.before)?;
        require(
            m.deployment == p.deployment
                && m.profile == profile_hash
                && m.snapshot.as_slice()
                    == p.base.bootstrap_state.account().merkle_root().as_slice()
                && m.phase <= 2
                && m.has_pending <= 1,
            Error::Profile,
        )?;
        m
    };
    let mut trees = Trees::new(&a.trees, &m, p)?;
    let mut sources = BTreeMap::new();
    let mut payments = BTreeMap::new();
    let mut used_recipients = BTreeSet::new();
    let mut checked_sources = BTreeSet::new();
    let mut unique = BTreeSet::new();
    let add_amount = |map: &mut BTreeMap<_, U256>, key, value| -> Result<()> {
        let v = map.entry(key).or_default();
        *v = plus(*v, value)?;
        Ok(())
    };
    // Non-payment actions cannot smuggle payout references; only source registration needs asset identities.
    require(
        a.op == Operation::Pay || a.native.payouts.is_empty(),
        Error::Output,
    )?;
    require(
        matches!(a.op, Operation::Pay | Operation::RegisterSources) || a.native.assets.is_empty(),
        Error::Asset,
    )?;
    require(
        matches!(a.op, Operation::Pay | Operation::RegisterClaims)
            || a.native.recipients.is_empty(),
        Error::Recipient,
    )?;
    match a.op {
        Operation::Begin => {
            let k = epoch_key(p.deployment, 0);
            trees.absent(EPOCHS, k)?;
            let mut epoch = p.initial_epoch;
            epoch.status = ACTIVE;
            trees.set(EPOCHS, k, &epoch)?;
            for l in &p.limits {
                let k = asset_key(p.deployment, l.asset);
                trees.absent(ASSETS, k)?;
                trees.set(
                    ASSETS,
                    k,
                    &AssetBudget {
                        limit: *l,
                        allocated: l.ceiling,
                        paid: 0.into(),
                    },
                )?;
            }
        }
        Operation::Pay => {
            require(m.phase == 2 && !a.records.is_empty(), Error::State)?;
            let e: Epoch = trees.get(EPOCHS, epoch_key(p.deployment, a.epoch))?;
            require(e.status == ACTIVE && e.id == a.epoch, Error::State)?;
            trees.roots[CLAIM_PLAN] = e.claims_root;
            for b in &a.records {
                let pay: Payment = decode(b)?;
                let c = &pay.claim;
                let ck = claim_key(p.deployment, c);
                require(
                    c.epoch == a.epoch && unique.insert(ck) && !pay.amount.is_zero(),
                    Error::Encoding,
                )?;
                require(trees.get::<Claim>(CLAIM_PLAN, ck)? == *c, Error::Proof)?;
                let r = recipient(&a.native, c, &mut used_recipients)?;
                let old = trees.original(PAID, ck)?;
                let n = if old.is_empty() {
                    U256::zero()
                } else {
                    let v: Paid = decode(old)?;
                    require(v.claim == ck && !v.amount.is_zero(), Error::State)?;
                    v.amount
                };
                let n = plus(n, pay.amount)?;
                require(n <= c.cap, Error::Balance)?;
                trees.set(
                    PAID,
                    ck,
                    &Paid {
                        claim: ck,
                        amount: n,
                    },
                )?;
                // Authenticate immutable source and payable lot against published roots.
                // Mutable budgets are created only when that source/lot is first requested.
                let src: records::Source = trees.get(SOURCE_PLAN, c.source)?;
                require(
                    source_key(p.deployment, &src) == c.source
                        && !src.baseline.is_zero()
                        && !version.protected(&src),
                    Error::Balance,
                )?;
                let lot: Lot = trees.get(LOT_PLAN, c.lot)?;
                require(
                    lot_key(p.deployment, &lot) == c.lot
                        && lot.source == c.source
                        && lot.kind == INITIAL
                        && !lot.amount.is_zero()
                        && lot.amount <= src.baseline
                        && c.cap <= lot.amount,
                    Error::Balance,
                )?;
                let mut reserve = if trees.original(RESERVES, c.lot)?.is_empty()
                    && !trees
                        .writes
                        .get(&RESERVES)
                        .is_some_and(|w| w.contains_key(&c.lot))
                {
                    Reserve {
                        lot,
                        remaining: lot.amount,
                    }
                } else {
                    trees.get::<Reserve>(RESERVES, c.lot)?
                };
                require(reserve.lot == lot, Error::State)?;
                reserve.remaining = minus(reserve.remaining, pay.amount)?;
                trees.set(RESERVES, c.lot, &reserve)?;
                let mut s = if trees.original(SOURCES, c.source)?.is_empty()
                    && !trees
                        .writes
                        .get(&SOURCES)
                        .is_some_and(|w| w.contains_key(&c.source))
                {
                    SourceBudget {
                        source: src,
                        allocated: src.baseline,
                        paid: 0.into(),
                        reserved: 0.into(),
                        lot_sum: src.baseline,
                        finalized: 1,
                    }
                } else {
                    trees.get::<SourceBudget>(SOURCES, c.source)?
                };
                require(s.source == src && s.finalized == 1, Error::State)?;
                if checked_sources.insert(c.source) {
                    source_identity(p, &a.native, &s.source, &mut reads, version)?;
                    require(
                        version.amount(read_native(
                            &a.native,
                            &mut reads,
                            version.balance_key(s.source.asset, s.source.address),
                        )?)? == minus(s.source.baseline, s.paid)?,
                        Error::Balance,
                    )?;
                }
                s.paid = plus(s.paid, pay.amount)?;
                require(s.paid <= s.allocated, Error::Balance)?;
                trees.set(SOURCES, c.source, &s)?;
                let ak = asset_key(p.deployment, s.source.asset);
                let mut asset: AssetBudget = trees.get(ASSETS, ak)?;
                require(
                    asset.limit.asset == s.source.asset
                        && asset.limit.type_hash == s.source.type_hash,
                    Error::Asset,
                )?;
                asset.paid = plus(asset.paid, pay.amount)?;
                require(asset.paid <= asset.allocated, Error::Balance)?;
                trees.set(ASSETS, ak, &asset)?;
                let v = sources
                    .entry((s.source.asset, s.source.address))
                    .or_insert(U256::zero());
                *v = plus(*v, pay.amount)?;
                add_amount(&mut payments, (r, s.source.asset), pay.amount)?;
            }
        }
        _ => return Err(Error::State),
    }
    require(
        used_recipients.len() == a.native.recipients.len(),
        Error::Recipient,
    )?;
    trees.finish(&mut m)?;
    // Reset only the completed pending registration tree; it is a scratch receipt, never paid state.
    if matches!(a.op, Operation::Activate | Operation::Abort) {
        m.roots[REGISTERED] = [0; 32];
    }
    m.sequence = increment(m.sequence)?;
    let metadata_value = commitment(&encode(&m));
    require(metadata_value != [0; 32], Error::State)?;
    let mut native_writes = BTreeMap::new();
    native_writes.insert(mk, metadata_value);
    for ((asset, address), n) in &sources {
        let bk = version.balance_key(*asset, *address);
        require(bk != mk, Error::Keys)?;
        native_writes.insert(
            bk,
            value(minus(
                version.amount(read_native(&a.native, &mut reads, bk)?)?,
                *n,
            )?),
        );
        if let Some(sk) = version.supply_key(*asset) {
            require(sk != mk, Error::Keys)?;
            let old = match native_writes.get(&sk) {
                Some(v) => *v,
                None => read_native(&a.native, &mut reads, sk)?,
            };
            native_writes.insert(sk, value(minus(version.amount(old)?, *n)?));
        }
    }
    Ok(Evaluation {
        metadata: m,
        tree_writes: trees.writes,
        native_writes,
        sources,
        payments,
        reads,
    })
}
pub fn verify(
    p: &SettlementProfile,
    profile_hash: Hash,
    cfg: &packed::RollupConfig,
    tx: &Transaction,
    a: &SettlementAction,
) -> Result<Outcome> {
    require(
        gw_hash::blake2b::hash(&codec::encode_profile(p)) == profile_hash
            && profile_hash != [0; 32],
        Error::Profile,
    )?;
    let input = tx
        .inputs
        .iter()
        .enumerate()
        .find(|(_, c)| {
            c.type_
                .as_ref()
                .is_some_and(|t| t.as_slice() == p.base.rollup.as_slice())
        })
        .ok_or(Error::Rollup)?;
    if a.op == Operation::Begin {
        require(
            tx.outpoints
                .get(input.0)
                .is_some_and(|o| o.as_slice() == p.base.bootstrap_outpoint.as_slice())
                && input.1.data.as_slice() == p.base.bootstrap_state.as_slice(),
            Error::Authority,
        )?;
    } else {
        require(
            input.1.lock.as_slice() == p.base.governance.as_slice(),
            Error::Authority,
        )?;
    }
    let e = evaluate(p, profile_hash, a)?;
    require(a.after == encode(&e.metadata), Error::State)?;
    let transition = SettlementTransition {
        admin: a.op != Operation::Pay,
        reads: e.reads,
        metadata_key: metadata_key(p.deployment, script_hash(&p.base.rollup)),
        metadata_value: commitment(&a.after),
        sources: e.sources,
        payments: e.payments,
    };
    verify_transition(&p.base, cfg, tx, &a.native, Some(&transition))
}
