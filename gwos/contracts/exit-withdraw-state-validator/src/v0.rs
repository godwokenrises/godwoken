//! Adapt the original V0 chain to the shared refund settlement rules.
//!
//! V0 has older configuration/custodian formats and a PoA-controlled rollup.
//! This module handles those differences so V0 and V1 can share the refund
//! accounting and native state/asset checks. It does not calculate the full
//! refund table: the CLI does that from the DB and reviewed policy. The contract
//! checks payments against the committed plan and the current on-chain state.
//!
//! # Start reading here
//!
//! - `verify()` is the main transaction verifier, called by `v0_main.rs` after
//!   loading cell dependencies, transaction cells and the witness action. It
//!   authenticates the deployment settings and original V0 configuration,
//!   checks the bootstrap/governance conditions, calls `evaluate()`, and passes
//!   the derived changes to the shared `verify_transition_version()` function.
//!   That final check validates native SMT updates, custodians, payout cells and
//!   asset conservation. CKB executes the input locks to verify signatures.
//! - `evaluate()` delegates to the shared settlement rules with `NativeVersion::V0`.
//!   These rules allow only Begin and Pay in epoch 0, verify entitlement proofs,
//!   and derive the updated refund metadata. Evaluation alone is not complete
//!   transaction verification; `verify()` also checks the actual cells and state.
//!
//! # Fixed settings versus changing refund state
//!
//! `Profile` means the fixed configuration for this refund deployment. It wraps
//! the shared settlement profile (rollup identity, authority, plan commitments
//! and limits) plus V0's PoA migration cells. `v0_main.rs` loads it from a cell
//! dependency on EVERY Begin and Pay. `decode_profile()` checks its bytes against
//! the hash pinned in the validator, so the caller cannot substitute settings.
//! Referencing this dependency does not consume it. Payment progress changes in
//! the refund state committed by the rollup account root, not in the profile.
//!
//! # What happens only once
//!
//! Begin must consume the exact configured bootstrap rollup cell. Its four
//! `Auxiliary` cells describe two PoA helper cells and two producer cells.
//! `check_profile()` validates this known setup; `check_auxiliary()` matches the
//! actual inputs and outputs. The helper locks move to the new governance lock,
//! the producer cells are recreated unchanged, and capacity/type/data are kept.
//! Later Pay transactions use the new governance lock and do not repeat this
//! migration. Only Begin passes auxiliary cells to the shared verifier.
//!
//! # V0 format helpers
//!
//! `config()` converts the old configuration into the shared in-memory form;
//! authentication still hashes the ORIGINAL V0 bytes. `custodian()` checks the
//! old lock format, rollup identity and deposit finality. Profile/action codecs
//! use V0-specific headers/tags so V1 payloads cannot be used here. The remaining
//! helpers handle bounded parsing, serialization and exact cell comparisons.
use crate::{
    native::NativeVersion,
    settlement::{self as s, records::*},
    verify::{require, SettlementTransition},
    *,
};
use alloc::{collections::BTreeSet, vec::Vec};
use gw_hash::blake2b::hash;
use gw_types::{packed, prelude::*};
pub const SECP: Hash = [
    155, 215, 224, 111, 62, 207, 75, 224, 242, 252, 210, 24, 139, 35, 241, 185, 252, 200, 142, 93,
    75, 101, 168, 99, 123, 23, 114, 59, 189, 163, 204, 232,
];
pub const POA: Hash = [
    237, 225, 153, 116, 95, 189, 158, 159, 51, 159, 29, 210, 233, 42, 88, 96, 32, 209, 17, 67, 43,
    251, 129, 166, 194, 177, 45, 220, 152, 86, 240, 186,
];
pub const POA_STATE: Hash = [
    28, 4, 223, 9, 217, 173, 237, 229, 191, 196, 15, 241, 163, 154, 58, 23, 252, 142, 41, 241, 92,
    86, 241, 107, 126, 72, 104, 12, 96, 14, 229, 172,
];
pub const ETH: Hash = [
    21, 99, 8, 13, 23, 91, 248, 221, 212, 74, 72, 232, 80, 206, 207, 12, 11, 69, 117, 131, 87, 86,
    235, 95, 253, 83, 173, 131, 9, 49, 185, 249,
];
const MAGIC: &[u8; 8] = b"GWV0SET1";
#[derive(Clone, Debug)]
/// One cell consumed and recreated during the initial PoA migration.
pub struct Auxiliary {
    pub outpoint: packed::OutPoint,
    pub before: Cell,
    pub after: Cell,
}
#[derive(Clone, Debug)]
/// Fixed deployment settings, loaded from a cell dependency on every Begin or Pay.
/// The validator pins their hash; only the auxiliary migration runs once, at Begin.
pub struct Profile {
    pub settlement: s::SettlementProfile,
    pub auxiliary: Vec<Auxiliary>,
}
/// Read exactly four bytes as a little-endian integer.
fn word(b: &[u8]) -> Result<u32> {
    Ok(u32::from_le_bytes(
        b.try_into().map_err(|_| Error::Encoding)?,
    ))
}
/// Check a Molecule table and borrow its fields without copying their contents.
pub fn table<const N: usize>(b: &[u8]) -> Result<[&[u8]; N]> {
    require(
        N > 0 && b.len() >= 4 * (N + 1) && word(&b[..4])? as usize == b.len(),
        Error::Encoding,
    )?;
    require(word(&b[4..8])? as usize == 4 * (N + 1), Error::Encoding)?;
    let mut out = [&[][..]; N];
    for (i, item) in out.iter_mut().enumerate() {
        let from = word(&b[4 + i * 4..8 + i * 4])? as usize;
        let to = if i + 1 == N {
            b.len()
        } else {
            word(&b[8 + i * 4..12 + i * 4])? as usize
        };
        require(from <= to && to <= b.len(), Error::Encoding)?;
        *item = &b[from..to];
    }
    Ok(out)
}
/// Convert the old list of script hashes into the form used by shared checks.
fn hashes(b: &[u8]) -> Result<Vec<packed::AllowedTypeHash>> {
    require(b.len() >= 4, Error::Encoding)?;
    let n = word(&b[..4])? as usize;
    require(n <= 64 && b.len() == 4 + n * 32, Error::Limit)?;
    b[4..]
        .chunks_exact(32)
        .map(|h| {
            Ok(packed::AllowedTypeHash::new_builder()
                .type_(1u8.into())
                .hash(packed::Byte32::from_slice(h).map_err(|_| Error::Encoding)?)
                .build())
        })
        .collect()
}
/// Read the old V0 configuration into the structure used by shared checks.
/// Authenticate its original bytes, never the converted representation.
pub fn config(b: &[u8]) -> Result<packed::RollupConfig> {
    let f = table::<14>(b)?;
    for x in &f[..8] {
        require(x.len() == 32, Error::Encoding)?;
    }
    for x in &f[8..11] {
        require(x.len() == 8, Error::Encoding)?;
    }
    require(f[11].len() == 1 && f[11][0] <= 100, Error::Encoding)?;
    let h = |i| packed::Byte32::from_slice(f[i]).map_err(|_| Error::Encoding);
    let u = |i| packed::Uint64::from_slice(f[i]).map_err(|_| Error::Encoding);
    Ok(packed::RollupConfig::new_builder()
        .l1_sudt_script_type_hash(h(0)?)
        .custodian_script_type_hash(h(1)?)
        .deposit_script_type_hash(h(2)?)
        .withdrawal_script_type_hash(h(3)?)
        .challenge_script_type_hash(h(4)?)
        .stake_script_type_hash(h(5)?)
        .l2_sudt_validator_script_type_hash(h(6)?)
        .burn_lock_hash(h(7)?)
        .required_staking_capacity(u(8)?)
        .challenge_maturity_blocks(u(9)?)
        .finality_blocks(u(10)?)
        .reward_burn_rate(f[11][0].into())
        .allowed_eoa_type_hashes(
            packed::AllowedTypeHashVec::new_builder()
                .set(hashes(f[12])?)
                .build(),
        )
        .allowed_contract_type_hashes(
            packed::AllowedTypeHashVec::new_builder()
                .set(hashes(f[13])?)
                .build(),
        )
        .build())
}
/// Accept only a finalized V0 custodian belonging to this rollup.
pub(crate) fn custodian(
    lock: &packed::Script,
    rollup: Hash,
    cfg: &packed::RollupConfig,
    state: &packed::GlobalStateReader<'_>,
) -> Result<()> {
    require(
        u8::from(lock.hash_type()) == 1
            && lock.code_hash().as_slice() == cfg.custodian_script_type_hash().as_slice(),
        Error::Cell,
    )?;
    let args = lock.as_reader().args().raw_data();
    require(args.len() >= 32 && args[..32] == rollup, Error::Cell)?;
    let f = table::<3>(&args[32..])?;
    require(f[1].len() == 32 && f[2].len() == 8, Error::Cell)?;
    let deposit = table::<3>(f[0])?;
    require(deposit[0].len() == 32 && deposit[2].len() == 8, Error::Cell)?;
    packed::ScriptReader::from_slice(deposit[1]).map_err(|_| Error::Cell)?;
    let n = u64::from_le_bytes(f[2].try_into().unwrap());
    let finalized: u64 = state.last_finalized_timepoint().unpack();
    require(n <= finalized && finalized < 1u64 << 63, Error::Finality)
}
/// Compare serialized values byte for byte.
fn same<T: Entity>(a: &T, b: &T) -> bool {
    a.as_slice() == b.as_slice()
}
/// Compare optional type scripts, including whether either script is absent.
fn types(a: &Option<packed::Script>, b: &Option<packed::Script>) -> bool {
    a.as_ref().map(|x| x.as_slice()) == b.as_ref().map(|x| x.as_slice())
}
/// Compare all cell contents; the outpoint is checked separately.
fn eq(a: &Cell, b: &Cell) -> bool {
    a.capacity == b.capacity
        && same(&a.lock, &b.lock)
        && types(&a.type_, &b.type_)
        && a.data == b.data
}
/// At Begin, match every configured migration input to one exact output.
/// Preserve capacity, type and data; permit only the configured lock transition.
pub(crate) fn check_auxiliary(
    aux: &[Auxiliary],
    tx: &Transaction,
    ri: usize,
    ro: usize,
    admin: bool,
    p: &crate::Profile,
) -> Result<(BTreeSet<usize>, BTreeSet<usize>)> {
    let mut ins = BTreeSet::new();
    let mut outs = BTreeSet::new();
    if aux.is_empty() {
        return Ok((ins, outs));
    }
    require(
        aux.len() <= 8
            && admin
            && same(&tx.outpoints[ri], &p.bootstrap_outpoint)
            && same(&tx.inputs[ri].lock, &p.bootstrap_lock),
        Error::Authority,
    )?;
    for x in aux {
        require(
            x.before.capacity == x.after.capacity
                && x.before.data == x.after.data
                && types(&x.before.type_, &x.after.type_)
                && (same(&x.after.lock, &x.before.lock) || same(&x.after.lock, &p.governance)),
            Error::Authority,
        )?;
        let i = tx
            .outpoints
            .iter()
            .position(|o| same(o, &x.outpoint))
            .ok_or(Error::Cell)?;
        require(
            i != ri && ins.insert(i) && eq(&tx.inputs[i], &x.before),
            Error::Cell,
        )?;
        let mut matches = tx
            .outputs
            .iter()
            .enumerate()
            .filter(|(_, c)| eq(c, &x.after))
            .map(|(i, _)| i);
        let index = matches.next().ok_or(Error::Cell)?;
        require(
            matches.next().is_none() && index != ro && outs.insert(index),
            Error::Cell,
        )?;
    }
    Ok((ins, outs))
}
/// Require the known V0 ETH/PoA setup, its four migration cells, and the new
/// single-signature governance lock. This does not verify transaction signatures.
fn check_profile(p: &Profile) -> Result<()> {
    let b = &p.settlement.base;
    require(
        b.eoa_codes == [ETH] && b.bootstrap_state.as_slice().len() == 190,
        Error::Profile,
    )?;
    require(
        b.bootstrap_lock.code_hash().as_slice() == POA
            && u8::from(b.bootstrap_lock.hash_type()) == 1,
        Error::Profile,
    )?;
    let args = b.bootstrap_lock.as_reader().args().raw_data();
    require(args.len() == 64 && p.auxiliary.len() == 4, Error::Profile)?;
    require(
        b.governance.code_hash().as_slice() == SECP
            && u8::from(b.governance.hash_type()) == 1
            && b.governance.as_reader().args().raw_data().len() == 20,
        Error::Profile,
    )?;
    let mut helpers = [None, None];
    let mut producers = Vec::new();
    let mut points = BTreeSet::new();
    for x in &p.auxiliary {
        require(
            points.insert(x.outpoint.as_slice()) && !same(&x.outpoint, &b.bootstrap_outpoint),
            Error::Profile,
        )?;
        if let Some(t) = &x.before.type_ {
            let tid = crate::v0::type_id_code();
            require(
                u8::from(t.hash_type()) == 1
                    && t.code_hash().as_slice() == tid
                    && x.before.lock.code_hash().as_slice() == POA_STATE
                    && u8::from(x.before.lock.hash_type()) == 1
                    && x.before.lock.as_reader().args().raw_data()
                        == script_hash(&b.bootstrap_lock),
                Error::Profile,
            )?;
            let a = t.as_reader().args().raw_data();
            let idx = if a == &args[..32] {
                0
            } else if a == &args[32..] {
                1
            } else {
                return Err(Error::Profile);
            };
            require(
                helpers[idx].is_none() && same(&x.after.lock, &b.governance),
                Error::Profile,
            )?;
            helpers[idx] = Some(x);
        } else {
            require(
                x.before.data.is_empty() && eq(&x.before, &x.after),
                Error::Profile,
            )?;
            producers.push(script_hash(&x.before.lock));
        }
        require(
            x.before.capacity == x.after.capacity
                && x.before.data == x.after.data
                && types(&x.before.type_, &x.after.type_),
            Error::Profile,
        )?;
    }
    let setup = &helpers[0].ok_or(Error::Profile)?.before.data;
    require(
        setup.len() == 76
            && setup[1..4] == [32, 2, 2]
            && setup[0] <= 1
            && helpers[1].ok_or(Error::Profile)?.before.data.len() == 22
            && producers.len() == 2
            && producers[0] != producers[1],
        Error::Profile,
    )?;
    require(
        producers
            .iter()
            .all(|h| h.as_slice() == &setup[12..44] || h.as_slice() == &setup[44..76]),
        Error::Profile,
    )
}
/// Return CKB's built-in TYPE_ID identifier for the PoA helper cells.
pub fn type_id_code() -> Hash {
    let mut h = [0; 32];
    h[25..].copy_from_slice(b"TYPE_ID");
    h
}
/// Append bytes with a four-byte length prefix.
fn put(b: &[u8], out: &mut Vec<u8>) {
    out.extend_from_slice(&(b.len() as u32).to_le_bytes());
    out.extend_from_slice(b);
}
/// Serialize the cell contents needed to bind a migration input or output.
fn cell(c: &Cell, out: &mut Vec<u8>) {
    out.extend_from_slice(&c.capacity.to_le_bytes());
    put(c.lock.as_slice(), out);
    put(c.type_.as_ref().map(|t| t.as_slice()).unwrap_or(&[]), out);
    put(&c.data, out);
}
/// Serialize deployment settings with a V0-only header for hashing and storage.
pub fn encode_profile(p: &Profile) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(MAGIC);
    put(&s::codec::encode_profile(&p.settlement), &mut out);
    out.extend_from_slice(&(p.auxiliary.len() as u32).to_le_bytes());
    for x in &p.auxiliary {
        out.extend_from_slice(x.outpoint.as_slice());
        cell(&x.before, &mut out);
        cell(&x.after, &mut out);
    }
    out
}
struct Read<'a>(&'a [u8]);
impl<'a> Read<'a> {
    /// Borrow the next bytes and advance the reader, rejecting truncated input.
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        require(n <= self.0.len(), Error::Encoding)?;
        let (a, b) = self.0.split_at(n);
        self.0 = b;
        Ok(a)
    }
    /// Read length-prefixed bytes, rejecting lengths above the supplied limit.
    fn bytes(&mut self, max: usize) -> Result<&'a [u8]> {
        let n = word(self.take(4)?)? as usize;
        require(n <= max, Error::Limit)?;
        self.take(n)
    }
    /// Decode one migration cell with bounded script and data sizes.
    fn cell(&mut self) -> Result<Cell> {
        let capacity = u64::from_le_bytes(self.take(8)?.try_into().unwrap());
        let lock =
            packed::Script::from_slice(self.bytes(MAX_SCRIPT)?).map_err(|_| Error::Encoding)?;
        let t = self.bytes(MAX_SCRIPT)?;
        let type_ = if t.is_empty() {
            None
        } else {
            Some(packed::Script::from_slice(t).map_err(|_| Error::Encoding)?)
        };
        Ok(Cell {
            capacity,
            lock,
            type_,
            data: self.bytes(MAX_CELL_DATA)?.to_vec(),
        })
    }
}
/// Authenticate the entire profile against the pinned hash, then decode and
/// validate it. Reject oversized input and any trailing bytes.
pub fn decode_profile(bytes: &[u8], expected: Hash) -> Result<Profile> {
    require(
        bytes.len() <= MAX_PROFILE && expected != [0; 32] && hash(bytes) == expected,
        Error::Profile,
    )?;
    let mut r = Read(bytes);
    require(r.take(8)? == MAGIC, Error::Encoding)?;
    let b = r.bytes(MAX_PROFILE)?;
    let settlement = s::codec::decode_profile(b, hash(b))?;
    let n = word(r.take(4)?)? as usize;
    require(n <= 8, Error::Limit)?;
    let mut auxiliary = Vec::with_capacity(n);
    for _ in 0..n {
        let outpoint = packed::OutPoint::from_slice(r.take(36)?).map_err(|_| Error::Encoding)?;
        auxiliary.push(Auxiliary {
            outpoint,
            before: r.cell()?,
            after: r.cell()?,
        });
    }
    require(r.0.is_empty(), Error::Encoding)?;
    let p = Profile {
        settlement,
        auxiliary,
    };
    check_profile(&p)?;
    Ok(p)
}
/// Encode a shared settlement action using the distinct V0 action tag.
pub fn encode_action(a: &s::SettlementAction) -> Vec<u8> {
    let mut b = s::codec::encode_action(a);
    b[..4].copy_from_slice(&s::codec::V0_ACTION_TAG.to_le_bytes());
    b
}
/// Decode only V0 settlement actions; reject V1 and unsupported operations.
pub fn decode_action(b: &[u8]) -> Result<s::SettlementAction> {
    s::codec::decode_action_tag(b, NativeVersion::V0)
}

/// Use the shared refund rules to derive the allowed V0 accounting changes.
/// Actual transaction cells and native state proofs are checked by verify().
pub fn evaluate(p: &Profile, ph: Hash, a: &s::SettlementAction) -> Result<s::Evaluation> {
    check_profile(p)?;
    s::evaluate_version(&p.settlement, ph, a, NativeVersion::V0)
}
/// Verify a complete V0 refund transaction: authenticate settings, enforce the
/// Begin/Pay authority rules, derive refunds, then check native state and assets.
pub fn verify(
    profile: &Profile,
    profile_hash: Hash,
    raw_config: &[u8],
    transaction: &Transaction,
    action: &s::SettlementAction,
) -> Result<Outcome> {
    // 1. Authenticate the fixed deployment settings. The caller cannot replace
    // the approved plan or authorities with a different profile.
    check_profile(profile)?;
    require(
        profile_hash != [0; 32] && hash(&encode_profile(profile)) == profile_hash,
        Error::Profile,
    )?;
    // 2. Read the old V0 configuration, but authenticate its ORIGINAL bytes.
    let normalized_config = config(raw_config)?;
    require(
        hash(raw_config) == profile.settlement.base.config_hash,
        Error::Profile,
    )?;
    // 3. Locate the rollup input and check which authority stage it belongs to.
    // The shared verifier below also checks rollup uniqueness and output rules.
    let base = &profile.settlement.base;
    let (rollup_input_index, rollup_input) = transaction
        .inputs
        .iter()
        .enumerate()
        .find(|(_, cell)| {
            cell.type_
                .as_ref()
                .is_some_and(|type_script| same(type_script, &base.rollup))
        })
        .ok_or(Error::Rollup)?;
    if action.op == s::Operation::Begin {
        // Begin must start from the exact approved bootstrap cell and state.
        require(
            transaction
                .outpoints
                .get(rollup_input_index)
                .is_some_and(|outpoint| same(outpoint, &base.bootstrap_outpoint))
                && rollup_input.data == base.bootstrap_state.as_slice(),
            Error::Authority,
        )?;
    } else {
        // Later refunds must use the new governance lock. CKB verifies its
        // signature; evaluate() below rejects operations other than Begin/Pay.
        require(same(&rollup_input.lock, &base.governance), Error::Authority)?;
    }
    // 4. Derive the refund result from the committed plan and supplied proofs.
    // Reject any caller-provided after-state that differs from our calculation.
    let evaluation = evaluate(profile, profile_hash, action)?;
    require(action.after == encode(&evaluation.metadata), Error::State)?;
    // 5. Pass only those derived changes to the native state verifier.
    // These bind the metadata commitment, source debits and recipient payments;
    // they are not an arbitrary write set supplied by the caller.
    let transition = SettlementTransition {
        admin: action.op != s::Operation::Pay,
        reads: evaluation.reads,
        metadata_key: metadata_key(profile.settlement.deployment, script_hash(&base.rollup)),
        metadata_value: commitment(&action.after),
        sources: evaluation.sources,
        payments: evaluation.payments,
    };
    // 6. Check the actual transaction: native SMT proofs and root update,
    // custodian inputs/outputs, permitted payouts and asset conservation.
    // PoA auxiliary cells participate only in the initial Begin migration.
    verify::verify_transition_version(
        base,
        &normalized_config,
        transaction,
        &action.native,
        Some(&transition),
        NativeVersion::V0,
        hash(raw_config),
        if action.op == s::Operation::Begin {
            &profile.auxiliary
        } else {
            &[]
        },
    )
}
