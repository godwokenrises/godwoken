use crate::proof_backend::ProofBackend;
use alloc::{
    collections::{BTreeMap, BTreeSet},
    vec::Vec,
};
use gw_common::state::*;
use gw_hash::blake2b::hash;
use gw_smt::smt::CompiledMerkleProof;
use gw_types::{core::Timepoint, packed, prelude::*, registry_address::RegistryAddress, U256};

use crate::native::NativeVersion;
pub type Hash = [u8; 32];
pub const SCALE: u64 = 10_000_000_000;
pub const MAX_RECIPIENTS: usize = 20;
pub const MAX_ASSETS: usize = 48;
pub const MAX_INPUTS: usize = 64;
pub const MAX_OUTPUTS: usize = 128;
pub const MAX_KEYS: usize = 1024;
pub const MAX_PROOF: usize = 256 * 1024;
pub const MAX_ACTION: usize = 384 * 1024;
pub const MAX_PROFILE: usize = 16 * 1024;
pub const MAX_CELL_DATA: usize = 4096;
pub const MAX_SCRIPT: usize = 2048;
pub const OMNI: Hash = [
    0x9b, 0x81, 0x97, 0x93, 0xa6, 0x44, 0x63, 0xae, 0xd7, 0x7c, 0x61, 0x5d, 0x6c, 0xb2, 0x26, 0xee,
    0xa5, 0x48, 0x7c, 0xcf, 0xc0, 0x78, 0x30, 0x43, 0xa5, 0x87, 0x25, 0x4c, 0xda, 0x2b, 0x6f, 0x26,
];
pub const MULTISIG: Hash = [
    0x5c, 0x50, 0x69, 0xeb, 0x08, 0x57, 0xef, 0xc6, 0x5e, 0x1b, 0xca, 0x0c, 0x07, 0xdf, 0x34, 0xc3,
    0x16, 0x63, 0xb3, 0x62, 0x2f, 0xd3, 0x87, 0x6c, 0x87, 0x63, 0x20, 0xfc, 0x96, 0x34, 0xe2, 0xa8,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i8)]
pub enum Error {
    Encoding = 5,
    Limit,
    Profile,
    Rollup,
    Authority,
    State,
    Cell,
    Finality,
    Recipient,
    Asset,
    Output,
    Conservation,
    Capacity,
    Keys,
    Proof,
    Balance,
    Supply,
    Overflow,
    Noop,
    Syscall,
}
pub type Result<T> = core::result::Result<T, Error>;
pub(crate) fn require(ok: bool, err: Error) -> Result<()> {
    if ok {
        Ok(())
    } else {
        Err(err)
    }
}
fn add(a: U256, b: U256) -> Result<U256> {
    a.checked_add(b).ok_or(Error::Overflow)
}
pub fn amount(v: Hash) -> U256 {
    U256::from_little_endian(&v)
}
pub fn value(n: U256) -> Hash {
    let mut b = [0; 32];
    n.to_little_endian(&mut b);
    b
}
pub fn script_hash(s: &packed::Script) -> Hash {
    hash(s.as_slice())
}
fn code(s: &packed::Script) -> Hash {
    s.code_hash().unpack()
}
fn is_type(s: &packed::Script) -> bool {
    u8::from(s.hash_type()) == 1
}

#[derive(Clone, Debug)]
pub struct Profile {
    pub rollup: packed::Script,
    pub config_hash: Hash,
    pub bootstrap_outpoint: packed::OutPoint,
    pub bootstrap_capacity: u64,
    pub bootstrap_lock: packed::Script,
    pub bootstrap_state: packed::GlobalState,
    pub governance: packed::Script,
    pub change: packed::Script,
    pub eoa_codes: Vec<Hash>,
    pub catalogue: Vec<(u32, Hash)>,
}
#[derive(Clone, Debug)]
pub struct Cell {
    pub capacity: u64,
    pub lock: packed::Script,
    pub type_: Option<packed::Script>,
    pub data: Vec<u8>,
}
impl Cell {
    pub fn minimum_capacity(&self) -> Result<u64> {
        // CKB occupied bytes: u64 capacity + Script fields (32+1+args), optional type, data.
        let mut n = 8usize
            .checked_add(33 + self.lock.as_reader().args().raw_data().len())
            .ok_or(Error::Overflow)?;
        if let Some(t) = &self.type_ {
            n = n
                .checked_add(33 + t.as_reader().args().raw_data().len())
                .ok_or(Error::Overflow)?;
        }
        n = n.checked_add(self.data.len()).ok_or(Error::Overflow)?;
        (n as u64).checked_mul(100_000_000).ok_or(Error::Overflow)
    }
}
#[derive(Clone, Debug)]
pub struct Transaction {
    pub inputs: Vec<Cell>,
    pub outputs: Vec<Cell>,
    pub outpoints: Vec<packed::OutPoint>,
}
#[derive(Clone, Debug)]
pub struct Recipient {
    pub address: [u8; 20],
    pub id: u32,
    pub script: packed::Script,
}
#[derive(Clone, Debug)]
pub struct Asset {
    pub id: u32,
    pub script: packed::Script,
}
#[derive(Clone, Debug)]
pub struct Payout {
    pub output: u32,
    pub recipient: u32,
    pub asset: u32,
}
#[derive(Clone, Debug)]
pub struct Action {
    pub recipients: Vec<Recipient>,
    pub assets: Vec<Asset>,
    pub payouts: Vec<Payout>,
    pub kv: Vec<(Hash, Hash)>,
    pub proof: CompiledMerkleProof,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outcome {
    pub root: Hash,
    pub debits: BTreeMap<(u32, u32), U256>,
    pub fee: u64,
    /// Governance CKB added to payout cells beyond the approved native debits.
    pub top_up_shannons: u64,
}
pub fn eth_lock(address: [u8; 20]) -> packed::Script {
    let mut args = alloc::vec![1];
    args.extend_from_slice(&address);
    args.push(0);
    packed::Script::new_builder()
        .code_hash(OMNI.pack())
        .hash_type(1u8.into())
        .args(args.pack())
        .build()
}
fn matches_eth_lock(lock: &packed::Script, address: &[u8; 20]) -> bool {
    let args = lock.as_reader().args().raw_data();
    is_type(lock)
        && code(lock) == OMNI
        && args.len() == 22
        && args[0] == 1
        && &args[1..21] == address
        && args[21] == 0
}
pub fn balance_key(id: u32, address: [u8; 20]) -> Hash {
    build_account_key(
        id,
        &build_sudt_key(
            SUDT_KEY_FLAG_BALANCE,
            &RegistryAddress::new(2, address.to_vec()),
        ),
    )
}
pub fn supply_key(id: u32) -> Hash {
    build_account_key(id, &SUDT_TOTAL_SUPPLY_KEY)
}
fn globals(cell: &Cell) -> Result<packed::GlobalStateReader<'_>> {
    packed::GlobalStateReader::from_slice(&cell.data).map_err(|_| Error::State)
}
fn finalized(cfg: &packed::RollupConfig, state: &packed::GlobalStateReader<'_>, time: u64) -> bool {
    match Timepoint::from_full_value(time) {
        Timepoint::BlockNumber(n) => {
            n.checked_add(cfg.finality_blocks().unpack())
                .is_some_and(|v| {
                    v <= u64::from_le_bytes(state.block().count().as_slice().try_into().unwrap())
                        .saturating_sub(1)
                })
        }
        Timepoint::Timestamp(n) => {
            matches!(Timepoint::from_full_value(state.last_finalized_timepoint().unpack()),Timepoint::Timestamp(f) if n<=f)
        }
    }
}
fn custodian(
    version: NativeVersion,
    lock: &packed::Script,
    rollup: Hash,
    cfg: &packed::RollupConfig,
    state: &packed::GlobalStateReader<'_>,
) -> Result<()> {
    if version == NativeVersion::V0 {
        return crate::v0::custodian(lock, rollup, cfg, state);
    }
    require(
        is_type(lock) && code(lock).as_slice() == cfg.custodian_script_type_hash().as_slice(),
        Error::Cell,
    )?;
    let args = lock.as_reader().args().raw_data();
    require(args.len() >= 32 && args[..32] == rollup, Error::Cell)?;
    let c = packed::CustodianLockArgsReader::from_slice(&args[32..]).map_err(|_| Error::Cell)?;
    require(
        finalized(cfg, state, c.deposit_finalized_timepoint().unpack()),
        Error::Finality,
    )
}
fn token(cell: &Cell, cfg: &packed::RollupConfig) -> Result<Option<(Hash, U256)>> {
    match &cell.type_ {
        None => {
            require(cell.data.is_empty(), Error::Cell)?;
            Ok(None)
        }
        Some(t) => {
            require(
                is_type(t)
                    && code(t).as_slice() == cfg.l1_sudt_script_type_hash().as_slice()
                    && t.as_reader().args().raw_data().len() == 32
                    && cell.data.len() == 16,
                Error::Asset,
            )?;
            Ok(Some((script_hash(t), U256::from_little_endian(&cell.data))))
        }
    }
}
fn insert_sum<K: Ord>(map: &mut BTreeMap<K, U256>, key: K, n: U256) -> Result<()> {
    let v = map.entry(key).or_default();
    *v = add(*v, n)?;
    Ok(())
}
fn pool_sum(map: &mut BTreeMap<Hash, U256>, cell: &Cell, cfg: &packed::RollupConfig) -> Result<()> {
    insert_sum(map, [0; 32], cell.capacity.into())?;
    if let Some((h, n)) = token(cell, cfg)? {
        require(h != [0; 32], Error::Asset)?;
        insert_sum(map, h, n)?;
    }
    Ok(())
}
fn ordered<T: Ord>(v: impl Iterator<Item = T>) -> bool {
    let mut previous = None;
    for item in v {
        if previous.as_ref().is_some_and(|p| p >= &item) {
            return false;
        }
        previous = Some(item);
    }
    true
}

/// Derived by the settlement verifier, never decoded from a caller-controlled write set.
pub(crate) struct SettlementTransition {
    pub admin: bool,
    pub reads: BTreeSet<Hash>,
    pub metadata_key: Hash,
    pub metadata_value: Hash,
    pub sources: BTreeMap<(u32, [u8; 20]), U256>,
    pub payments: BTreeMap<(u32, u32), U256>,
}
pub(crate) fn verify_transition(
    profile: &Profile,
    cfg: &packed::RollupConfig,
    tx: &Transaction,
    a: &Action,
    settlement: &SettlementTransition,
) -> Result<Outcome> {
    verify_transition_version(
        profile,
        cfg,
        tx,
        a,
        settlement,
        NativeVersion::V1,
        hash(cfg.as_slice()),
        &[],
    )
}
pub(crate) fn verify_transition_version(
    profile: &Profile,
    cfg: &packed::RollupConfig,
    tx: &Transaction,
    a: &Action,
    settlement: &SettlementTransition,
    version: NativeVersion,
    config_hash: Hash,
    auxiliary: &[crate::v0::Auxiliary],
) -> Result<Outcome> {
    // Step 1: Bound transaction/witness sizes before doing expensive work.
    // Begin may have no recipients or payouts; a payment must include them.
    let admin = settlement.admin;
    require(
        !tx.inputs.is_empty()
            && tx.inputs.len() <= MAX_INPUTS
            && !tx.outputs.is_empty()
            && tx.outputs.len() <= MAX_OUTPUTS
            && tx.inputs.len() == tx.outpoints.len(),
        Error::Limit,
    )?;
    require(
        (admin || !a.recipients.is_empty())
            && a.recipients.len() <= MAX_RECIPIENTS
            && (admin || !a.assets.is_empty())
            && a.assets.len() <= MAX_ASSETS
            && (admin || !a.payouts.is_empty())
            && a.payouts.len() <= MAX_OUTPUTS
            && !a.kv.is_empty()
            && a.kv.len() <= MAX_KEYS
            && !a.proof.0.is_empty()
            && a.proof.0.len() <= MAX_PROOF,
        Error::Limit,
    )?;
    // Step 2: Require a canonical order with no duplicate addresses, asset IDs,
    // payout tuples or state keys. Later lookups rely on these unique entries.
    require(
        ordered(a.recipients.iter().map(|r| r.address))
            && ordered(a.assets.iter().map(|s| s.id))
            && ordered(a.payouts.iter().map(|p| (p.recipient, p.asset, p.output))),
        Error::Encoding,
    )?;
    require(ordered(a.kv.iter().map(|(k, _)| *k)), Error::Keys)?;
    // Step 3: Validate the configured rollup, governance and asset catalogue.
    // CKB is asset 1 with a zero type hash; other asset identities must be unique.
    // Accepted EOA scripts must also be allowed by the original rollup config.
    require(
        is_type(&profile.rollup)
            && profile.rollup.as_reader().args().raw_data().len() == 32
            && config_hash == profile.config_hash
            && profile.bootstrap_state.rollup_config_hash().as_slice() == profile.config_hash,
        Error::Profile,
    )?;
    require(
        (code(&profile.governance) == MULTISIG
            || (version == NativeVersion::V0 && code(&profile.governance) == crate::v0::SECP))
            && is_type(&profile.governance)
            && profile.governance.as_reader().args().raw_data().len() == 20
            && profile.governance.as_slice() != profile.bootstrap_lock.as_slice(),
        Error::Profile,
    )?;
    require(
        !profile.catalogue.is_empty()
            && profile.catalogue.len() <= MAX_ASSETS
            && ordered(profile.catalogue.iter().map(|x| x.0))
            && profile
                .catalogue
                .iter()
                .filter(|(_, h)| *h == [0; 32])
                .map(|(id, _)| *id)
                .eq(core::iter::once(1)),
        Error::Profile,
    )?;
    require(
        profile
            .catalogue
            .iter()
            .map(|(_, h)| *h)
            .collect::<BTreeSet<_>>()
            .len()
            == profile.catalogue.len()
            && !profile.eoa_codes.is_empty()
            && profile.eoa_codes.len() <= 16
            && ordered(profile.eoa_codes.iter().copied()),
        Error::Profile,
    )?;
    for h in &profile.eoa_codes {
        require(
            cfg.allowed_eoa_type_hashes()
                .into_iter()
                .any(|x| u8::from(x.type_()) == 1 && x.hash().as_slice() == h),
            Error::Profile,
        )?;
    }
    // Step 4: Reject duplicate inputs and malformed or underfunded cells.
    // Each cell must have enough CKB to occupy its own scripts and data.
    require(
        tx.outpoints
            .iter()
            .map(|o| o.as_slice())
            .collect::<BTreeSet<_>>()
            .len()
            == tx.inputs.len(),
        Error::Cell,
    )?;
    for cell in tx.inputs.iter().chain(&tx.outputs) {
        require(
            cell.data.len() <= MAX_CELL_DATA
                && cell.lock.as_slice().len() <= MAX_SCRIPT
                && cell
                    .type_
                    .as_ref()
                    .is_none_or(|s| s.as_slice().len() <= MAX_SCRIPT),
            Error::Limit,
        )?;
        require(cell.capacity >= cell.minimum_capacity()?, Error::Capacity)?;
    }
    // Step 5: Require exactly one rollup input and one rollup output.
    // Preserve its capacity and put the output under the configured governance
    // lock. Accept only the exact bootstrap state or an existing governance lock.
    // CKB executes input locks separately to validate their signatures.
    let mut rin = tx.inputs.iter().enumerate().filter(|(_, c)| {
        c.type_
            .as_ref()
            .is_some_and(|s| s.as_slice() == profile.rollup.as_slice())
    });
    let mut rout = tx.outputs.iter().enumerate().filter(|(_, c)| {
        c.type_
            .as_ref()
            .is_some_and(|s| s.as_slice() == profile.rollup.as_slice())
    });
    let (ri, ic) = rin.next().ok_or(Error::Rollup)?;
    let (ro, oc) = rout.next().ok_or(Error::Rollup)?;
    require(rin.next().is_none() && rout.next().is_none(), Error::Rollup)?;
    let before = globals(ic)?;
    let after = globals(oc)?;
    require(
        ic.capacity == oc.capacity && oc.lock.as_slice() == profile.governance.as_slice(),
        Error::Authority,
    )?;
    if ic.lock.as_slice() == profile.bootstrap_lock.as_slice() {
        require(
            tx.outpoints[ri].as_slice() == profile.bootstrap_outpoint.as_slice()
                && ic.capacity == profile.bootstrap_capacity
                && before.as_slice() == profile.bootstrap_state.as_slice(),
            Error::Authority,
        )?;
    } else {
        require(
            ic.lock.as_slice() == profile.governance.as_slice(),
            Error::Authority,
        )?;
    }
    // Step 6: Freeze the chain's non-account state. Only the account SMT root
    // may change; block history, finality, account count and config must stay put.
    require(
        before.status().as_slice()[0] == 0
            && before.version().as_slice()[0] == 1
            && before.rollup_config_hash().as_slice() == profile.config_hash,
        Error::State,
    )?;
    let oldroot: Hash = before.account().merkle_root().unpack();
    let newroot: Hash = after.account().merkle_root().unpack();
    require(
        before.rollup_config_hash().as_slice() == after.rollup_config_hash().as_slice()
            && before.account().count().as_slice() == after.account().count().as_slice()
            && before.block().as_slice() == after.block().as_slice()
            && before.reverted_block_root().as_slice() == after.reverted_block_root().as_slice()
            && before.tip_block_hash().as_slice() == after.tip_block_hash().as_slice()
            && before.tip_block_timestamp().as_slice() == after.tip_block_timestamp().as_slice()
            && before.last_finalized_timepoint().as_slice()
                == after.last_finalized_timepoint().as_slice()
            && before.status().as_slice() == after.status().as_slice()
            && before.version().as_slice() == after.version().as_slice(),
        Error::State,
    )?;
    // Step 7: Validate the configured custodian change lock for this protocol.
    // V1 uses the canonical zero-deposit change form; V0 has its older format.
    let count: u32 = before.account().count().unpack();
    let rollup = script_hash(&profile.rollup);
    custodian(version, &profile.change, rollup, cfg, &before).map_err(|_| Error::Profile)?;
    if version == NativeVersion::V1 {
        let ch = profile.change.as_reader().args().raw_data();
        let ca =
            packed::CustodianLockArgsReader::from_slice(&ch[32..]).map_err(|_| Error::Profile)?;
        require(
            ca.deposit_block_hash().as_slice() == [0; 32]
                && u64::from_le_bytes(
                    ca.deposit_finalized_timepoint()
                        .as_slice()
                        .try_into()
                        .unwrap(),
                ) == 0,
            Error::Profile,
        )?;
    }
    // Step 8: Track every state key needed by this transaction. Values read here
    // are still UNTRUSTED witness data until the old-root proof check in step 15.
    // Start with the state reads already required by the settlement rules.
    // The codec/verifier requires sorted unique keys; borrow them instead of copying into a tree.
    let mut expected = settlement.reads.clone();
    let mut read = |k: Hash| -> Result<Hash> {
        expected.insert(k);
        a.kv.binary_search_by_key(&k, |(key, _)| *key)
            .map(|index| a.kv[index].1)
            .map_err(|_| Error::Keys)
    };
    // Step 9: Bind each recipient to its L2 identity or controller descriptor.
    // EOA script args must contain exactly this rollup and the ETH address.
    // V0's L2 balance address comes from the script hash; the payout ETH address
    // still comes from the EOA script args. Controller authorization follows below.
    for r in &a.recipients {
        if crate::control::is_descriptor(&r.script) {
            crate::control::validate(&r.script, rollup, r.id)?;
            require(
                r.address == crate::control::identity(&r.script),
                Error::Recipient,
            )?;
            continue;
        }
        require(
            r.id < count
                && r.address != [0; 20]
                && is_type(&r.script)
                && profile.eoa_codes.contains(&code(&r.script)),
            Error::Recipient,
        )?;
        let args = r.script.as_reader().args().raw_data();
        require(
            args.len() == 52 && args[..32] == rollup && args[32..] == r.address,
            Error::Recipient,
        )?;
        let sh = script_hash(&r.script);
        let address = if version == NativeVersion::V0 {
            sh[..20].try_into().unwrap()
        } else {
            r.address
        };
        version.identity(r.id, sh, address, count, &mut read)?;
    }
    // Step 10: Bind each requested L2 asset to its authenticated account script
    // and the approved L1 token type hash. Payments must also account for CKB.
    let mut assets = BTreeMap::new();
    for asset in &a.assets {
        require(
            asset.id < count
                && is_type(&asset.script)
                && code(&asset.script).as_slice()
                    == cfg.l2_sudt_validator_script_type_hash().as_slice(),
            Error::Asset,
        )?;
        let args = asset.script.as_reader().args().raw_data();
        require(args.len() == 64 && args[..32] == rollup, Error::Asset)?;
        let h: Hash = args[32..].try_into().map_err(|_| Error::Asset)?;
        require(profile.catalogue.contains(&(asset.id, h)), Error::Asset)?;
        require(
            read(build_account_field_key(
                asset.id,
                GW_ACCOUNT_SCRIPT_HASH_TYPE,
            ))? == script_hash(&asset.script),
            Error::Asset,
        )?;
        assets.insert(asset.id, h);
    }
    require(admin || assets.get(&1) == Some(&[0; 32]), Error::Asset)?;
    // Step 11: Inspect the actual payout cells, not just the requested amounts.
    // EOA outputs must use the exact allowed ETH lock; controller outputs are
    // checked by authorize() below. Reject reused outputs or wrong token types.
    // Count each payout cell's full capacity as CKB paid, including token cells.
    let mut paid = BTreeMap::new();
    let mut debits = BTreeMap::new();
    let mut payout_indices = BTreeSet::new();
    let mut recipients_used = BTreeSet::new();
    let mut assets_used = BTreeSet::new();
    assets_used.insert(1);
    for p in &a.payouts {
        let r = a
            .recipients
            .get(p.recipient as usize)
            .ok_or(Error::Output)?;
        let c = tx.outputs.get(p.output as usize).ok_or(Error::Output)?;
        require(
            payout_indices.insert(p.output as usize)
                && p.output as usize != ro
                && ((crate::control::is_descriptor(&r.script))
                    || matches_eth_lock(&c.lock, &r.address)),
            Error::Output,
        )?;
        recipients_used.insert(p.recipient);
        assets_used.insert(p.asset);
        let h = *assets.get(&p.asset).ok_or(Error::Asset)?;
        match token(c, cfg)? {
            None => require(p.asset == 1, Error::Asset)?,
            Some((actual, n)) => {
                require(p.asset != 1 && actual == h && !n.is_zero(), Error::Asset)?;
                insert_sum(&mut debits, (p.recipient, p.asset), n)?;
            }
        }
        insert_sum(
            &mut debits,
            (p.recipient, 1),
            U256::from(c.capacity) * U256::from(version.scale()),
        )?;
        pool_sum(&mut paid, c, cfg)?;
    }
    require(
        if admin {
            a.payouts.is_empty()
        } else {
            recipients_used.len() == a.recipients.len() && assets_used.len() == assets.len()
        },
        Error::Output,
    )?;
    // Step 12: Validate controller approval cells and their payout bindings,
    // then validate any V0 bootstrap migration cells. Track these separately so
    // they cannot be mistaken for custodians or fee-sponsor cells.
    let (control_inputs, control_outputs) =
        crate::control::authorize(tx, a, &payout_indices, rollup, !admin)?;
    let (aux_inputs, aux_outputs) =
        crate::v0::check_auxiliary(auxiliary, tx, ri, ro, admin, profile)?;
    // Step 13: Classify every remaining cell and account for all pool assets.
    // Governance-owned plain CKB funds fees. Every other input must be a valid
    // custodian; every other output must be valid custodian change. Thus an
    // unlisted payout cannot hide among unrelated outputs.
    let mut ins = BTreeMap::new();
    let mut outs = BTreeMap::new();
    let mut ni = 0;
    let mut no = 0;
    let mut sponsor_in = 0u64;
    let mut sponsor_out = 0u64;
    let mut ns = 0;
    for (i, c) in tx.inputs.iter().enumerate() {
        if i == ri || aux_inputs.contains(&i) || control_inputs.contains(&i) {
            continue;
        }
        if c.lock.as_slice() == profile.governance.as_slice() {
            require(c.type_.is_none() && c.data.is_empty(), Error::Cell)?;
            sponsor_in = sponsor_in.checked_add(c.capacity).ok_or(Error::Overflow)?;
            ns += 1;
        } else {
            custodian(version, &c.lock, rollup, cfg, &before)?;
            pool_sum(&mut ins, c, cfg)?;
            ni += 1;
        }
    }
    for (i, c) in tx.outputs.iter().enumerate() {
        if i == ro
            || payout_indices.contains(&i)
            || aux_outputs.contains(&i)
            || control_outputs.contains(&i)
        {
            continue;
        }
        if c.lock.as_slice() == profile.governance.as_slice() {
            require(c.type_.is_none() && c.data.is_empty(), Error::Cell)?;
            sponsor_out = sponsor_out.checked_add(c.capacity).ok_or(Error::Overflow)?;
        } else {
            // V0 change must reuse a custodian lock present among the inputs;
            // V1 change must use the exact configured canonical lock.
            if version == NativeVersion::V0 {
                custodian(version, &c.lock, rollup, cfg, &before)?;
                require(
                    tx.inputs.iter().enumerate().any(|(j, input)| {
                        j != ri
                            && !aux_inputs.contains(&j)
                            && input.lock.as_slice() == c.lock.as_slice()
                    }),
                    Error::Cell,
                )?;
            } else {
                require(c.lock.as_slice() == profile.change.as_slice(), Error::Cell)?;
            }
            pool_sum(&mut outs, c, cfg)?;
            no += 1;
        }
    }
    // Begin does not move pool assets. Pay must consume and recreate custodians.
    // Fees and any explicit payout capacity top-up come from governance CKB.
    // Custodians fund only the approved entitlements, never sponsor gifts.
    require(ns > 0, Error::Authority)?;
    require(
        if admin {
            ni == 0 && no == 0
        } else {
            ni > 0 && no > 0
        },
        Error::Cell,
    )?;
    let sponsor_spent = sponsor_in.checked_sub(sponsor_out).ok_or(Error::Capacity)?;
    for (h, n) in paid {
        insert_sum(&mut outs, h, n)?;
    }
    let mut debits = debits;
    let mut top_up_shannons = 0u64;
    let s = settlement;
    if !admin {
        for ((recipient, asset), actual) in &debits {
            if *asset != 1 {
                require(
                    s.payments.get(&(*recipient, *asset)) == Some(actual),
                    Error::Balance,
                )?;
            }
        }
        for ((recipient, asset), expected) in &s.payments {
            if *asset != 1 {
                require(
                    debits.get(&(*recipient, *asset)) == Some(expected),
                    Error::Balance,
                )?;
            }
        }
        let scale = U256::from(version.scale());
        for ((recipient, asset), actual) in &debits {
            if *asset != 1 {
                continue;
            }
            let expected = s
                .payments
                .get(&(*recipient, 1))
                .copied()
                .unwrap_or_default();
            require(*actual >= expected, Error::Balance)?;
            let extra = *actual - expected;
            require(extra % scale == U256::zero(), Error::Capacity)?;
            let extra = extra / scale;
            require(extra <= U256::from(u64::MAX), Error::Overflow)?;
            top_up_shannons = top_up_shannons
                .checked_add(extra.low_u64())
                .ok_or(Error::Overflow)?;
        }
        for ((recipient, asset), expected) in &s.payments {
            if *asset == 1 {
                require(
                    debits
                        .get(&(*recipient, 1))
                        .is_some_and(|actual| actual >= expected),
                    Error::Balance,
                )?;
            }
        }
        debits = s.payments.clone();
    }
    let fee = sponsor_spent
        .checked_sub(top_up_shannons)
        .ok_or(Error::Capacity)?;
    if top_up_shannons != 0 {
        let ckb = outs.get_mut(&[0; 32]).ok_or(Error::Conservation)?;
        *ckb = ckb
            .checked_sub(U256::from(top_up_shannons))
            .ok_or(Error::Conservation)?;
    }
    // Keep zero-valued type identities: every consumed token type must be returned or paid.
    require(ins == outs, Error::Conservation)?;
    // Step 14: Derive the only permitted native SMT writes.
    // Match evaluated entitlements, debit approved sources and commit settlement metadata.
    let s = settlement;
    require(debits == s.payments, Error::Balance)?;
    require(
        !s.reads.contains(&[0; 32]) && s.reads.contains(&s.metadata_key),
        Error::Keys,
    )?;
    let mut sums = BTreeMap::new();
    let mut writes = BTreeMap::new();
    for ((id, address), n) in &s.sources {
        let key = version.balance_key(*id, *address);
        require(
            key != s.metadata_key && version.supply_key(*id) != Some(s.metadata_key),
            Error::Keys,
        )?;
        let old = version.amount(read(key)?)?;
        writes.insert(key, value(old.checked_sub(*n).ok_or(Error::Balance)?));
        insert_sum(&mut sums, *id, *n)?;
    }
    writes.insert(s.metadata_key, s.metadata_value);
    // Reduce recorded supply where this protocol defines a supply key, by the
    // same amount removed from its source balances. Reject supply underflow.
    for (id, n) in sums {
        let Some(key) = version.supply_key(id) else {
            continue;
        };
        let old = version.amount(read(key)?)?;
        writes.insert(key, value(old.checked_sub(n).ok_or(Error::Supply)?));
    }
    // Step 15: Authenticate all witness reads against the INPUT account root.
    // No unused witness keys are allowed, and an invalid proof rejects everything
    // calculated above. This is what makes the earlier balance/identity reads valid.
    // Every requested key exists; equal cardinality excludes unused witness keys.
    require(expected.len() == a.kv.len(), Error::Keys)?;
    let checked = a
        .proof
        .backend_root(
            a.kv.iter()
                .map(|(k, v)| ((*k).into(), (*v).into()))
                .collect(),
        )
        .map_err(|_| Error::Proof)?;
    require(checked.as_slice() == oldroot, Error::Proof)?;
    // Step 16: Apply only our derived writes and recompute the OUTPUT root with
    // the same proof. All other proven leaves stay unchanged. Require the root
    // to match the output rollup and reject a transaction that changes nothing.
    // The SMT API consumes its leaf vector. These two leaf buffers are necessary;
    // the proof bytes and original key/value slice are reused by reference.
    let root: Hash = a
        .proof
        .backend_root(
            a.kv.iter()
                .map(|(k, v)| ((*k).into(), (*writes.get(k).unwrap_or(v)).into()))
                .collect(),
        )
        .map_err(|_| Error::Proof)?
        .into();
    require(root == newroot, Error::State)?;
    require(root != oldroot, Error::Noop)?;
    Ok(Outcome {
        root,
        debits,
        fee,
        top_up_shannons,
    })
}
