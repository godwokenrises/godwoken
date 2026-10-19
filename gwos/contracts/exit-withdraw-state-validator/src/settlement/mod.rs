//! Settlement-only consensus. All writes are derived from typed, authenticated records.
//! verify() binds the transaction; evaluate_version() handles Begin/Pay and derives counters.
use crate::proof_backend::ProofBackend;
pub mod codec;
pub mod physical;
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
use gw_smt::smt::CompiledMerkleProof;
use gw_types::{packed, prelude::*, U256};
use records::*;

pub const EPOCHS: usize = 0;
pub const PAID: usize = 1;
pub const SOURCES: usize = 2;
pub const RESERVES: usize = 3;
pub const ASSETS: usize = 4;
pub const SOURCE_PLAN: usize = 5;
pub const LOT_PLAN: usize = 6;
pub const CLAIM_PLAN: usize = 7;
pub const RECORD_KINDS: usize = 8;
pub const FORMAT_VERSION: u32 = 3;
pub const MAX_RECORDS: usize = 64;
pub const MAX_TREE_KEYS: usize = 256;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Operation {
    Begin = 0,
    Pay = 7,
}
impl Operation {
    pub fn from_u32(n: u32) -> Result<Self> {
        Ok(match n {
            0 => Self::Begin,
            7 => Self::Pay,
            _ => return Err(Error::Encoding),
        })
    }
}
#[derive(Clone, Debug)]
pub struct SettlementProfile {
    pub base: Profile,
    pub deployment: Hash,
    pub manifest: Hash,
    pub state_root: Hash,
    pub source_count: u64,
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
    proof: &'a TreeProof,
    pages: BTreeMap<Hash, physical::Page>,
    locations: BTreeMap<(usize, Hash), Hash>,
    globals: BTreeMap<(usize, Hash), Hash>,
    used: BTreeSet<Hash>,
    empty: Vec<u8>,
    root: Hash,
    deployment: Hash,
    op: Operation,
    writes: BTreeMap<usize, BTreeMap<Hash, Vec<u8>>>,
}
impl<'a> Trees<'a> {
    fn new(
        proofs: &'a [TreeProof],
        m: &Metadata,
        p: &SettlementProfile,
        op: Operation,
    ) -> Result<Self> {
        require(
            proofs.len() == 1 && proofs[0].kind == physical::STATE,
            Error::Proof,
        )?;
        let proof = &proofs[0];
        // Index supplied page preimages here; finish() authenticates them against state_root.
        require(
            !proof.leaves.is_empty()
                && proof.leaves.len() <= MAX_TREE_KEYS
                && proof.leaves.windows(2).all(|w| w[0].0 < w[1].0)
                && proof.proof.0.len() <= MAX_PROOF,
            Error::Limit,
        )?;
        let mut pages = BTreeMap::new();
        let mut locations = BTreeMap::new();
        for (k, raw) in &proof.leaves {
            match physical::namespace(*k)? {
                16 => {
                    let page = physical::unpack_page(raw)?;
                    for ((kind, key), _) in &page {
                        if matches!(*kind, SOURCE_PLAN | LOT_PLAN | CLAIM_PLAN) {
                            require(locations.insert((*kind, *key), *k).is_none(), Error::Keys)?;
                        }
                    }
                    pages.insert(*k, page);
                }
                32 | 48 => require(raw.len() <= 1024, Error::Limit)?,
                _ => return Err(Error::Keys),
            }
        }
        let mut globals = BTreeMap::new();
        globals.insert(
            (EPOCHS, epoch_key(p.deployment, 0)),
            physical::epoch_state_key(0),
        );
        for l in &p.limits {
            globals.insert(
                (ASSETS, asset_key(p.deployment, l.asset)),
                physical::asset_state_key(l.asset),
            );
        }
        Ok(Self {
            proof,
            pages,
            locations,
            globals,
            used: BTreeSet::new(),
            empty: Vec::new(),
            root: m.state_root,
            deployment: p.deployment,
            op,
            writes: BTreeMap::new(),
        })
    }
    fn location(&self, kind: usize, key: Hash) -> Result<Hash> {
        if matches!(kind, EPOCHS | ASSETS) {
            return self.globals.get(&(kind, key)).copied().ok_or(Error::Keys);
        }
        self.locations
            .get(&(physical::static_kind(kind)?, key))
            .copied()
            .ok_or(Error::Keys)
    }
    fn original(&mut self, kind: usize, key: Hash) -> Result<&[u8]> {
        let loc = self.location(kind, key)?;
        self.used.insert(loc);
        if matches!(kind, EPOCHS | ASSETS) {
            let i = self
                .proof
                .leaves
                .binary_search_by_key(&loc, |v| v.0)
                .map_err(|_| Error::Keys)?;
            return Ok(&self.proof.leaves[i].1);
        }
        let page = self.pages.get(&loc).ok_or(Error::Proof)?;
        if matches!(kind, SOURCE_PLAN | LOT_PLAN | CLAIM_PLAN) {
            let raw = page.get(&(kind, key)).ok_or(Error::Keys)?;
            let account = u32::from_le_bytes(loc[..4].try_into().map_err(|_| Error::Keys)?);
            let source = match kind {
                SOURCE_PLAN => {
                    let src: records::Source = decode(raw)?;
                    require(
                        source_key(self.deployment, &src) == key && src.account == account,
                        Error::Keys,
                    )?;
                    None
                }
                LOT_PLAN => Some(decode::<Lot>(raw)?.source),
                CLAIM_PLAN => Some(decode::<Claim>(raw)?.source),
                _ => None,
            };
            if let Some(source) = source {
                let sloc = self.location(SOURCE_PLAN, source)?;
                let src: records::Source = decode(
                    self.pages
                        .get(&sloc)
                        .and_then(|p| p.get(&(SOURCE_PLAN, source)))
                        .ok_or(Error::Keys)?,
                )?;
                require(src.account == account, Error::Keys)?;
            }
        }
        // An absent counter is proven by its containing page, located through its static parent.
        Ok(page.get(&(kind, key)).unwrap_or(&self.empty))
    }
    fn get<T: Field>(&mut self, t: usize, k: Hash) -> Result<T> {
        self.original(t, k)?;
        // Claims sharing a lot/source/asset must see updates already staged in this action.
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
        // Only evaluator-derived counters may change; Begin may also insert the initial epoch.
        require(
            physical::mutable(t) || (self.op == Operation::Begin && t == EPOCHS),
            Error::State,
        )?;
        self.original(t, k)?;
        self.writes.entry(t).or_default().insert(k, encode(v));
        Ok(())
    }
    fn finish(&self, m: &mut Metadata) -> Result<()> {
        // Authenticate every supplied physical leaf, rejecting unused proof leaves.
        require(self.used.len() == self.proof.leaves.len(), Error::Keys)?;
        let old = self
            .proof
            .leaves
            .iter()
            .map(|(k, v)| ((*k).into(), commitment(v).into()))
            .collect();
        let checked: Hash = self
            .proof
            .proof
            .backend_root(old)
            .map_err(|_| Error::Proof)?
            .into();
        require(checked == self.root, Error::Proof)?;
        // Preserve complete pages so static records and unrelated counters remain unchanged.
        let mut pages = self.pages.clone();
        let mut global = BTreeMap::new();
        for (kind, writes) in &self.writes {
            require(
                physical::mutable(*kind) || (self.op == Operation::Begin && *kind == EPOCHS),
                Error::State,
            )?;
            for (key, value) in writes {
                let loc = self.location(*kind, *key)?;
                if matches!(*kind, EPOCHS | ASSETS) {
                    global.insert(loc, value.clone());
                } else {
                    pages
                        .get_mut(&loc)
                        .ok_or(Error::Keys)?
                        .insert((*kind, *key), value.clone());
                }
            }
        }
        let new = self
            .proof
            .leaves
            .iter()
            .map(|(k, v)| {
                let raw = if let Some(page) = pages.get(k) {
                    let raw = physical::pack_page(page);
                    require(
                        raw.len() <= physical::MAX_PAGE_BYTES
                            && page.len() <= physical::PAGE_ITEMS * 2,
                        Error::Limit,
                    )?;
                    raw
                } else {
                    global.get(k).cloned().unwrap_or_else(|| v.clone())
                };
                Ok(((*k).into(), commitment(&raw).into()))
            })
            .collect::<Result<_>>()?;
        // Reuse the same multiproof: only derived leaves change; all other branches stay fixed.
        m.state_root = self
            .proof
            .proof
            .backend_root(new)
            .map_err(|_| Error::Proof)?
            .into();
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
            && p.state_root != [0; 32]
            && p.source_count > 0
            && p.reserve_count > 0,
        Error::Profile,
    )?;
    let e = &p.initial_epoch;
    require(
        e.id == 0
            && e.status == PENDING
            && e.claim_count > 0
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
    require(a.epoch == 0, Error::State)?;
    require(
        a.records.len() <= MAX_RECORDS && a.records.iter().all(|r| r.len() <= 1024),
        Error::Limit,
    )?;
    let mk = metadata_key(p.deployment, script_hash(&p.base.rollup));
    let mut reads = BTreeSet::new();
    // Native witness values become trusted only when verify_transition checks the native proof.
    let old_native_meta = read_native(&a.native, &mut reads, mk)?;
    let mut m = if a.op == Operation::Begin {
        // A successful Begin commits metadata, so initialization cannot be repeated.
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
            state_root: p.state_root,
        }
    } else {
        // Pay must resume the metadata committed by the current native root.
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
                && m.sequence > 0,
            Error::Profile,
        )?;
        m
    };
    let mut trees = Trees::new(&a.trees, &m, p, a.op)?;
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
    // Begin cannot carry payment references or native asset/recipient identities.
    require(
        a.op == Operation::Pay || a.native.payouts.is_empty(),
        Error::Output,
    )?;
    require(
        a.op == Operation::Pay || a.native.assets.is_empty(),
        Error::Asset,
    )?;
    require(
        a.op == Operation::Pay || a.native.recipients.is_empty(),
        Error::Recipient,
    )?;
    match a.op {
        Operation::Begin => {
            // Activate the frozen plan and initialize asset counters; account counters stay lazy.
            let k = epoch_key(p.deployment, 0);
            trees.absent(EPOCHS, k)?;
            let mut epoch = p.initial_epoch;
            epoch.status = ACTIVE;
            trees.set(EPOCHS, k, &epoch)?;
            for l in &p.limits {
                let k = asset_key(p.deployment, l.asset);
                trees.absent(ASSETS, k)?;
                trees.set(ASSETS, k, &AssetBudget { paid: 0.into() })?;
            }
        }
        Operation::Pay => {
            require(!a.records.is_empty(), Error::State)?;
            let e: Epoch = trees.get(EPOCHS, epoch_key(p.deployment, a.epoch))?;
            require(e.status == ACTIVE && e.id == a.epoch, Error::State)?;

            // Per claim: cap -> payable lot -> source balance/budget -> global asset ceiling.
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
                // No stored Paid means zero; an explicit stored zero is noncanonical.
                let n = if old.is_empty() {
                    U256::zero()
                } else {
                    let v: Paid = decode(old)?;
                    require(!v.amount.is_zero(), Error::State)?;
                    v.amount
                };
                let n = plus(n, pay.amount)?;
                require(n <= c.cap, Error::Balance)?;
                trees.set(PAID, ck, &Paid { amount: n })?;
                // Authenticate immutable source and payable lot within the published state root.
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
                        remaining: lot.amount,
                    }
                } else {
                    trees.get::<Reserve>(RESERVES, c.lot)?
                };
                reserve.remaining = minus(reserve.remaining, pay.amount)?;
                trees.set(RESERVES, c.lot, &reserve)?;
                let mut s = if trees.original(SOURCES, c.source)?.is_empty()
                    && !trees
                        .writes
                        .get(&SOURCES)
                        .is_some_and(|w| w.contains_key(&c.source))
                {
                    SourceBudget { paid: 0.into() }
                } else {
                    trees.get::<SourceBudget>(SOURCES, c.source)?
                };
                if checked_sources.insert(c.source) {
                    // Check the pre-transaction balance once; later claims use the staged budget.
                    source_identity(p, &a.native, &src, &mut reads, version)?;
                    require(
                        version.amount(read_native(
                            &a.native,
                            &mut reads,
                            version.balance_key(src.asset, src.address),
                        )?)? == minus(src.baseline, s.paid)?,
                        Error::Balance,
                    )?;
                }
                s.paid = plus(s.paid, pay.amount)?;
                require(s.paid <= src.baseline, Error::Balance)?;
                trees.set(SOURCES, c.source, &s)?;
                let ak = asset_key(p.deployment, src.asset);
                let mut asset: AssetBudget = trees.get(ASSETS, ak)?;
                let limit = p
                    .limits
                    .iter()
                    .find(|l| l.asset == src.asset)
                    .ok_or(Error::Asset)?;
                require(
                    limit.asset == src.asset && limit.type_hash == src.type_hash,
                    Error::Asset,
                )?;
                asset.paid = plus(asset.paid, pay.amount)?;
                require(asset.paid <= limit.ceiling, Error::Balance)?;
                trees.set(ASSETS, ak, &asset)?;
                let v = sources
                    .entry((src.asset, src.address))
                    .or_insert(U256::zero());
                *v = plus(*v, pay.amount)?;
                add_amount(&mut payments, (r, src.asset), pay.amount)?;
            }
        }
    }
    require(
        used_recipients.len() == a.native.recipients.len(),
        Error::Recipient,
    )?;
    trees.finish(&mut m)?;
    m.sequence = increment(m.sequence)?;
    let metadata_value = commitment(&encode(&m));
    require(metadata_value != [0; 32], Error::State)?;
    let mut native_writes = BTreeMap::new();
    // Commit the settlement root inside native metadata, alongside source and supply debits.
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
        // Consume the exact bootstrap cell once; a later rollup output cannot restart Begin.
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
    // The caller declares after-state bytes but cannot choose the resulting root or counters.
    require(a.after == encode(&e.metadata), Error::State)?;
    let transition = SettlementTransition {
        admin: a.op != Operation::Pay,
        reads: e.reads,
        metadata_key: metadata_key(p.deployment, script_hash(&p.base.rollup)),
        metadata_value: commitment(&a.after),
        sources: e.sources,
        payments: e.payments,
    };
    // Complete authorization: native SMT updates, actual payouts, custody and input-lock rules.
    // CKB executes input locks for signatures; this layer checks their required identities.
    verify_transition(&p.base, cfg, tx, &a.native, &transition)
}
