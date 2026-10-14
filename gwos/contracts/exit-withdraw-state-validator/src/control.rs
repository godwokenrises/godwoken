//! Root-committed controller descriptors; these are metadata, never executable locks.
//! Each distinct controller authorizes the whole transaction through a standard
//! mainnet Ethereum OmniLock input, returned unchanged. No signature is trusted
//! merely because it appears in a witness or is counted by a coordinator.
use crate::*;
use alloc::{collections::BTreeSet, vec::Vec};
use gw_types::{packed, prelude::*};

pub fn descriptor_code() -> Hash {
    gw_hash::blake2b::hash(b"GW_REFUND_CONTROL_DESCRIPTOR_V1")
}
pub fn is_descriptor(s: &packed::Script) -> bool {
    s.code_hash().as_slice() == descriptor_code() && u8::from(s.hash_type()) == 1
}
pub fn descriptor(
    rollup: Hash,
    account: u32,
    threshold: u8,
    owners: &[[u8; 20]],
) -> Result<packed::Script> {
    let mut args = Vec::with_capacity(38 + owners.len() * 20);
    args.extend_from_slice(&rollup);
    args.extend_from_slice(&account.to_le_bytes());
    args.push(threshold);
    args.push(u8::try_from(owners.len()).map_err(|_| Error::Limit)?);
    for owner in owners {
        args.extend_from_slice(owner);
    }
    let script = packed::Script::new_builder()
        .code_hash(descriptor_code().pack())
        .hash_type(1u8.into())
        .args(args.pack())
        .build();
    validate(&script, rollup, account)?;
    Ok(script)
}
pub fn validate(s: &packed::Script, rollup: Hash, account: u32) -> Result<()> {
    let a = s.as_reader().args().raw_data();
    if !is_descriptor(s)
        || a.len() < 38
        || a[..32] != rollup
        || a[32..36] != account.to_le_bytes()
        || a[36] == 0
        || a[36] > a[37]
        || a[37] > 16
        || a.len() != 38 + 20 * a[37] as usize
    {
        return Err(Error::Recipient);
    }
    let mut previous: Option<&[u8]> = None;
    for owner in a[38..].chunks_exact(20) {
        if owner == [0; 20] || previous.is_some_and(|p| p >= owner) {
            return Err(Error::Recipient);
        }
        previous = Some(owner);
    }
    Ok(())
}
pub fn identity(s: &packed::Script) -> [u8; 20] {
    script_hash(s)[..20].try_into().unwrap()
}
pub fn ethereum_owner(s: &packed::Script) -> Option<[u8; 20]> {
    let a = s.as_reader().args().raw_data();
    if s.code_hash().as_slice() != OMNI
        || u8::from(s.hash_type()) != 1
        || a.len() != 22
        || a[0] != 1
        || a[21] != 0
    {
        return None;
    }
    Some(a[1..21].try_into().unwrap())
}
/// Return input/output indices which must be omitted from custody accounting.
/// These cells are exact pass-throughs; they cannot fund payouts, fees or change.
pub fn authorize(
    tx: &Transaction,
    action: &Action,
    payouts: &BTreeSet<usize>,
    rollup: Hash,
    paying: bool,
) -> Result<(BTreeSet<usize>, BTreeSet<usize>)> {
    let mut ins = BTreeSet::new();
    let mut outs = BTreeSet::new();
    for r in &action.recipients {
        if !is_descriptor(&r.script) {
            continue;
        }
        validate(&r.script, rollup, r.id)?;
        if r.address != identity(&r.script) {
            return Err(Error::Recipient);
        }
        if !paying {
            continue;
        }
        let args = r.script.as_reader().args().raw_data();
        let mut seen = BTreeSet::new();
        for (i, c) in tx.inputs.iter().enumerate() {
            let Some(owner) = ethereum_owner(&c.lock) else {
                continue;
            };
            if !args[38..].chunks_exact(20).any(|o| o == owner) {
                continue;
            }
            if c.type_.is_some() || !c.data.is_empty() || !seen.insert(owner) {
                return Err(Error::Authority);
            }
            // The same controller may authorize several selected contracts with one input.
            if ins.insert(i) {
                let j = tx
                    .outputs
                    .iter()
                    .enumerate()
                    .find(|(j, o)| {
                        !payouts.contains(j)
                            && !outs.contains(j)
                            && o.capacity == c.capacity
                            && o.lock.as_slice() == c.lock.as_slice()
                            && o.type_.is_none()
                            && o.data.is_empty()
                    })
                    .map(|(j, _)| j)
                    .ok_or(Error::Authority)?;
                outs.insert(j);
            }
        }
        if seen.len() < args[36] as usize {
            return Err(Error::Authority);
        }
        // One destination per control account, jointly approved by all required input locks.
        let mut destination: Option<&packed::Script> = None;
        for p in &action.payouts {
            if action
                .recipients
                .get(p.recipient as usize)
                .is_some_and(|x| x.address == r.address)
            {
                let lock = &tx.outputs.get(p.output as usize).ok_or(Error::Output)?.lock;
                if destination.is_some_and(|s| s.as_slice() != lock.as_slice()) {
                    return Err(Error::Output);
                }
                destination = Some(lock);
            }
        }
        if destination.is_none() {
            return Err(Error::Output);
        }
    }
    Ok((ins, outs))
}
