//! Refund-only dispatch: v1 accepts tag 5, v0 accepts tag 6, both inner version 2.
//! There is no fallback to the historical RollupAction or direct-account exit parsers.
use super::*;
use crate::wire as w;
use gw_hash::blake2b::hash;
pub const V1_ACTION_TAG: u32 = 5;
pub const V0_ACTION_TAG: u32 = 6;

fn b(v: &[u8]) -> w::Bytes {
    w::Bytes::new_builder()
        .set(v.iter().map(|x| (*x).into()).collect())
        .build()
}
fn h(v: Hash) -> w::Byte32 {
    w::Byte32::from_slice(&v).unwrap()
}
fn n(v: u32) -> w::Uint32 {
    w::Uint32::from_slice(&v.to_le_bytes()).unwrap()
}
fn u(v: u64) -> w::Uint64 {
    w::Uint64::from_slice(&v.to_le_bytes()).unwrap()
}
fn rn(b: &[u8]) -> Result<u32> {
    Ok(u32::from_le_bytes(
        b.try_into().map_err(|_| Error::Encoding)?,
    ))
}
fn ru(b: &[u8]) -> Result<u64> {
    Ok(u64::from_le_bytes(
        b.try_into().map_err(|_| Error::Encoding)?,
    ))
}
fn rh(b: &[u8]) -> Result<Hash> {
    b.try_into().map_err(|_| Error::Encoding)
}
pub fn encode_profile(p: &SettlementProfile) -> Vec<u8> {
    w::SettlementProfile::new_builder()
        .version(n(2))
        .base(b(&crate::codec::encode_profile(&p.base)))
        .deployment(h(p.deployment))
        .manifest(h(p.manifest))
        .source_root(h(p.source_root))
        .source_count(u(p.source_count))
        .reserve_root(h(p.reserve_root))
        .reserve_count(u(p.reserve_count))
        .initial_epoch(b(&encode(&p.initial_epoch)))
        .limits(
            w::SettlementRecords::new_builder()
                .set(p.limits.iter().map(|v| b(&encode(v))).collect())
                .build(),
        )
        .build()
        .as_slice()
        .to_vec()
}
pub fn decode_profile(bytes: &[u8], expected: Hash) -> Result<SettlementProfile> {
    require(bytes.len() <= MAX_PROFILE, Error::Limit)?;
    require(
        expected != [0; 32] && hash(bytes) == expected,
        Error::Profile,
    )?;
    let p = w::SettlementProfileReader::from_slice(bytes).map_err(|_| Error::Encoding)?;
    require(
        rn(p.version().as_slice())? == 2 && p.limits().len() <= MAX_ASSETS,
        Error::Encoding,
    )?;
    let base = p.base().raw_data();
    let p = SettlementProfile {
        base: crate::codec::decode_profile(&base, hash(&base))?,
        deployment: rh(p.deployment().as_slice())?,
        manifest: rh(p.manifest().as_slice())?,
        source_root: rh(p.source_root().as_slice())?,
        source_count: ru(p.source_count().as_slice())?,
        reserve_root: rh(p.reserve_root().as_slice())?,
        reserve_count: ru(p.reserve_count().as_slice())?,
        initial_epoch: decode(&p.initial_epoch().raw_data())?,
        limits: p
            .limits()
            .iter()
            .map(|v| decode(&v.raw_data()))
            .collect::<Result<Vec<_>>>()?,
    };
    check_profile(&p)?;
    Ok(p)
}
pub fn encode_action(a: &SettlementAction) -> Vec<u8> {
    let body = w::SettlementAction::new_builder()
        .version(n(2))
        .operation(n(a.op as u32))
        .epoch(u(a.epoch))
        .before(b(&a.before))
        .after(b(&a.after))
        .records(
            w::SettlementRecords::new_builder()
                .set(a.records.iter().map(|v| b(v)).collect())
                .build(),
        )
        .trees(
            w::SettlementProofs::new_builder()
                .set(
                    a.trees
                        .iter()
                        .map(|t| {
                            w::SettlementProof::new_builder()
                                .kind(n(t.kind as u32))
                                .leaves(
                                    w::SettlementLeaves::new_builder()
                                        .set(
                                            t.leaves
                                                .iter()
                                                .map(|(k, v)| {
                                                    w::SettlementLeaf::new_builder()
                                                        .key(h(*k))
                                                        .record(b(v))
                                                        .build()
                                                })
                                                .collect(),
                                        )
                                        .build(),
                                )
                                .proof(b(&t.proof.0))
                                .build()
                        })
                        .collect(),
                )
                .build(),
        )
        .native(b(&crate::codec::encode_action(&a.native)))
        .build();
    [V1_ACTION_TAG.to_le_bytes().as_slice(), body.as_slice()].concat()
}
pub fn decode_action(bytes: &[u8]) -> Result<SettlementAction> {
    decode_action_tag(bytes, NativeVersion::V1)
}
pub(crate) fn decode_action_tag(bytes: &[u8], version: NativeVersion) -> Result<SettlementAction> {
    let tag = match version {
        NativeVersion::V1 => V1_ACTION_TAG,
        NativeVersion::V0 => V0_ACTION_TAG,
    };
    require(bytes.len() <= MAX_ACTION, Error::Limit)?;
    require(
        bytes.len() >= 4 && bytes[..4] == tag.to_le_bytes(),
        Error::Encoding,
    )?;
    let a = w::SettlementActionReader::from_slice(&bytes[4..]).map_err(|_| Error::Encoding)?;
    require(rn(a.version().as_slice())? == 2, Error::Encoding)?;
    // Reject unknown operations before copying records, proofs or native witness buffers.
    // The exhaustive enum contains refund lifecycle operations only; never dispatch legacy actions.
    let op = Operation::from_u32(rn(a.operation().as_slice())?)?;
    require(
        a.records().len() <= MAX_RECORDS
            && a.trees().len() <= 12
            && a.before().raw_data().len() <= 1024
            && a.after().raw_data().len() <= 1024,
        Error::Limit,
    )?;
    let records = a
        .records()
        .iter()
        .map(|v| {
            require(v.raw_data().len() <= 1024, Error::Limit)?;
            Ok(v.raw_data().to_vec())
        })
        .collect::<Result<Vec<_>>>()?;
    let trees = a
        .trees()
        .iter()
        .map(|t| {
            require(
                t.leaves().len() <= MAX_TREE_KEYS && t.proof().raw_data().len() <= MAX_PROOF,
                Error::Limit,
            )?;
            Ok(TreeProof {
                kind: rn(t.kind().as_slice())? as usize,
                leaves: t
                    .leaves()
                    .iter()
                    .map(|v| {
                        require(v.record().raw_data().len() <= 1024, Error::Limit)?;
                        Ok((rh(v.key().as_slice())?, v.record().raw_data().to_vec()))
                    })
                    .collect::<Result<Vec<_>>>()?,
                proof: CompiledMerkleProof(t.proof().raw_data().to_vec()),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(SettlementAction {
        op,
        epoch: ru(a.epoch().as_slice())?,
        before: a.before().raw_data().to_vec(),
        after: a.after().raw_data().to_vec(),
        records,
        trees,
        native: crate::codec::decode_action(&a.native().raw_data())?,
    })
}
