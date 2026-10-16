//! Native transition payload embedded only in the V3 settlement action.
use crate::{wire, *};
use alloc::vec::Vec;
use gw_hash::blake2b::hash;
use gw_types::{packed, prelude::*};

fn u32le(v: &[u8]) -> Result<u32> {
    Ok(u32::from_le_bytes(
        v.try_into().map_err(|_| Error::Encoding)?,
    ))
}
fn script(v: &[u8]) -> Result<packed::Script> {
    if v.len() > MAX_SCRIPT {
        return Err(Error::Limit);
    }
    packed::Script::from_slice(v).map_err(|_| Error::Encoding)
}
pub fn decode_action(bytes: &[u8]) -> Result<Action> {
    if bytes.len() > MAX_ACTION {
        return Err(Error::Limit);
    }
    let a = wire::NativeTransitionReader::from_slice(bytes).map_err(|_| Error::Encoding)?;
    if u32le(a.version().as_slice())? != crate::settlement::FORMAT_VERSION {
        return Err(Error::Encoding);
    }
    if a.recipients().len() > MAX_RECIPIENTS
        || a.assets().len() > MAX_ASSETS
        || a.payouts().len() > MAX_OUTPUTS
        || a.kv_state().len() > MAX_KEYS
        || a.proof().raw_data().len() > MAX_PROOF
    {
        return Err(Error::Limit);
    }
    Ok(Action {
        recipients: a
            .recipients()
            .iter()
            .map(|r| {
                Ok(Recipient {
                    address: r
                        .address()
                        .as_slice()
                        .try_into()
                        .map_err(|_| Error::Encoding)?,
                    id: u32le(r.account_id().as_slice())?,
                    script: script(&r.script().raw_data())?,
                })
            })
            .collect::<Result<Vec<_>>>()?,
        assets: a
            .assets()
            .iter()
            .map(|x| {
                Ok(Asset {
                    id: u32le(x.id().as_slice())?,
                    script: script(&x.script().raw_data())?,
                })
            })
            .collect::<Result<Vec<_>>>()?,
        payouts: a
            .payouts()
            .iter()
            .map(|p| {
                Ok(Payout {
                    output: u32le(p.output_index().as_slice())?,
                    recipient: u32le(p.recipient_index().as_slice())?,
                    asset: u32le(p.asset_id().as_slice())?,
                })
            })
            .collect::<Result<Vec<_>>>()?,
        kv: a
            .kv_state()
            .iter()
            .map(|kv| {
                Ok((
                    kv.key()
                        .as_slice()
                        .try_into()
                        .map_err(|_| Error::Encoding)?,
                    kv.value()
                        .as_slice()
                        .try_into()
                        .map_err(|_| Error::Encoding)?,
                ))
            })
            .collect::<Result<Vec<_>>>()?,
        proof: gw_smt::smt::CompiledMerkleProof(a.proof().raw_data().to_vec()),
    })
}
pub fn decode_profile(bytes: &[u8], expected: Hash) -> Result<Profile> {
    if bytes.len() > MAX_PROFILE {
        return Err(Error::Limit);
    }
    if expected == [0; 32] || hash(bytes) != expected {
        return Err(Error::Profile);
    }
    let p = wire::SettlementBaseReader::from_slice(bytes).map_err(|_| Error::Encoding)?;
    if p.catalogue().len() > MAX_ASSETS || p.eth_eoa_codes().len() > 16 {
        return Err(Error::Limit);
    }
    Ok(Profile {
        rollup: script(&p.rollup_script().raw_data())?,
        config_hash: p
            .config_hash()
            .as_slice()
            .try_into()
            .map_err(|_| Error::Encoding)?,
        bootstrap_outpoint: packed::OutPoint::from_slice(&p.bootstrap_outpoint().raw_data())
            .map_err(|_| Error::Encoding)?,
        bootstrap_capacity: u64::from_le_bytes(
            p.bootstrap_capacity()
                .as_slice()
                .try_into()
                .map_err(|_| Error::Encoding)?,
        ),
        bootstrap_lock: script(&p.bootstrap_lock().raw_data())?,
        bootstrap_state: packed::GlobalState::from_slice(&p.bootstrap_state().raw_data())
            .map_err(|_| Error::Encoding)?,
        governance: script(&p.governance_lock().raw_data())?,
        change: script(&p.custodian_change_lock().raw_data())?,
        eoa_codes: p
            .eth_eoa_codes()
            .iter()
            .map(|h| h.as_slice().try_into().map_err(|_| Error::Encoding))
            .collect::<Result<Vec<_>>>()?,
        catalogue: p
            .catalogue()
            .iter()
            .map(|a| {
                Ok((
                    u32le(a.id().as_slice())?,
                    a.type_hash()
                        .as_slice()
                        .try_into()
                        .map_err(|_| Error::Encoding)?,
                ))
            })
            .collect::<Result<Vec<_>>>()?,
    })
}

// Canonical encoders shared with off-chain transaction construction.
use crate::wire as w;
fn wb(v: &[u8]) -> w::Bytes {
    w::Bytes::new_builder()
        .set(v.iter().map(|x| (*x).into()).collect())
        .build()
}
fn w32(v: Hash) -> w::Byte32 {
    w::Byte32::from_slice(&v).unwrap()
}
fn wn(v: u32) -> w::Uint32 {
    w::Uint32::from_slice(&v.to_le_bytes()).unwrap()
}
pub fn encode_action(a: &Action) -> Vec<u8> {
    let b = w::NativeTransition::new_builder()
        .version(wn(crate::settlement::FORMAT_VERSION))
        .recipients(
            w::Recipients::new_builder()
                .set(
                    a.recipients
                        .iter()
                        .map(|r| {
                            w::Recipient::new_builder()
                                .address(w::Byte20::from_slice(&r.address).unwrap())
                                .account_id(wn(r.id))
                                .script(wb(r.script.as_slice()))
                                .build()
                        })
                        .collect(),
                )
                .build(),
        )
        .assets(
            w::Assets::new_builder()
                .set(
                    a.assets
                        .iter()
                        .map(|x| {
                            w::Asset::new_builder()
                                .id(wn(x.id))
                                .script(wb(x.script.as_slice()))
                                .build()
                        })
                        .collect(),
                )
                .build(),
        )
        .payouts(
            w::Payouts::new_builder()
                .set(
                    a.payouts
                        .iter()
                        .map(|p| {
                            w::Payout::new_builder()
                                .output_index(wn(p.output))
                                .recipient_index(wn(p.recipient))
                                .asset_id(wn(p.asset))
                                .build()
                        })
                        .collect(),
                )
                .build(),
        )
        .kv_state(
            w::KVs::new_builder()
                .set(
                    a.kv.iter()
                        .map(|(k, v)| w::KV::new_builder().key(w32(*k)).value(w32(*v)).build())
                        .collect(),
                )
                .build(),
        )
        .proof(wb(&a.proof.0))
        .build();
    b.as_slice().to_vec()
}
pub fn encode_profile(p: &Profile) -> Vec<u8> {
    w::SettlementBase::new_builder()
        .rollup_script(wb(p.rollup.as_slice()))
        .config_hash(w32(p.config_hash))
        .bootstrap_outpoint(wb(p.bootstrap_outpoint.as_slice()))
        .bootstrap_capacity(w::Uint64::from_slice(&p.bootstrap_capacity.to_le_bytes()).unwrap())
        .bootstrap_lock(wb(p.bootstrap_lock.as_slice()))
        .bootstrap_state(wb(p.bootstrap_state.as_slice()))
        .governance_lock(wb(p.governance.as_slice()))
        .custodian_change_lock(wb(p.change.as_slice()))
        .eth_eoa_codes(
            w::Hashes::new_builder()
                .set(p.eoa_codes.iter().map(|h| w32(*h)).collect())
                .build(),
        )
        .catalogue(
            w::Catalogue::new_builder()
                .set(
                    p.catalogue
                        .iter()
                        .map(|(id, h)| {
                            w::AssetSpec::new_builder()
                                .id(wn(*id))
                                .type_hash(w32(*h))
                                .build()
                        })
                        .collect(),
                )
                .build(),
        )
        .build()
        .as_slice()
        .to_vec()
}
