//! Offline lock-group signing. No RPC, key persistence, or transaction submission.
use super::*;
use crate::withdraw::build::Skeleton;
use secp256k1::{
    ecdsa::{RecoverableSignature, RecoveryId},
    Message, Secp256k1, SecretKey,
};
use serde::{Deserialize, Serialize};
use sha3::{Digest, Keccak256};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SigningGroup {
    pub lock_hash: String,
    pub input_indices: Vec<usize>,
    pub kind: String,
    /// For Ethereum this is the 32-byte personal_sign payload, not its prefixed digest.
    pub message: String,
    pub signing_digest: String,
    pub required_signatures: usize,
    pub require_first_n: usize,
    pub identities: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SigningRequest {
    pub transaction_hash: String,
    pub transaction_with_placeholders_hash: String,
    pub groups: Vec<SigningGroup>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Signature {
    pub lock_hash: String,
    pub signature: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Signatures {
    pub transaction_hash: String,
    pub signatures: Vec<Signature>,
}
pub(crate) fn omni(sig: &[u8]) -> Vec<u8> {
    let total = 20 + sig.len();
    let mut b = Vec::with_capacity(total);
    for n in [total, 16, total, total, sig.len()] {
        b.extend_from_slice(&(n as u32).to_le_bytes());
    }
    b.extend_from_slice(sig);
    b
}
fn witness_lock(tx: &mut Transaction, i: usize, lock: Vec<u8>) -> Result<()> {
    let w =
        packed::WitnessArgs::from_slice(&bytes(tx.witnesses.get(i).context("Missing witness")?)?)?;
    tx.witnesses[i] = hx(w
        .as_builder()
        .lock(Some(gw_types::bytes::Bytes::from(lock)).pack())
        .build()
        .as_slice());
    Ok(())
}
/// Re-derive groups from actual input locks, never trust a skeleton's explanatory signing_groups.
pub fn requests(d: &Deployment, sk: &Skeleton) -> Result<SigningRequest> {
    let p = Profile::load(d)?;
    let tx = &sk.transaction;
    ensure!(
        tx.input_points()?
            == sk
                .input_cells
                .iter()
                .map(|c| c.out_point.clone())
                .collect::<Vec<_>>(),
        "Input resolution differs"
    );
    let config = sk
        .dependency_cells
        .iter()
        .find(|c| hash(&bytes(&c.output_data).unwrap_or_default()) == p.base.config_hash)
        .context("Missing config dependency")?;
    let cfg = bytes(&config.output_data)?;
    let a = replay::action_for(tx, &p)?;
    let outcome = p.verify(&cfg, &chain::resolved(tx, &sk.input_cells)?, &a)?;
    ensure!(
        outcome.fee == d.fee_shannons,
        "Signing fee differs from deployment manifest"
    );
    ensure!(
        sk.profile_hash == d.profile_hash,
        "Signing profile differs from deployment manifest"
    );

    let mut groups = BTreeMap::<String, Vec<usize>>::new();
    // Custodian locks execute without signatures; rollup and fee inputs are the signing groups.
    for (i, cell) in sk.input_cells.iter().enumerate() {
        let lock = cell.output.lock.packed()?;
        if lock_template(d, &p, &lock)?.is_some() {
            groups.entry(cell.output.lock.hash()?).or_default().push(i);
        }
    }
    let mut out = Vec::new();
    for (lock_hash, indices) in groups {
        let lock = sk.input_cells[indices[0]].output.lock.packed()?;
        let (kind, placeholder, required, first, identities) =
            lock_template(d, &p, &lock)?.context("Unknown signing lock")?;
        let w = packed::WitnessArgs::from_slice(&bytes(&tx.witnesses[indices[0]])?)?;
        ensure!(
            w.lock()
                .to_opt()
                .context("Missing signing placeholder")?
                .raw_data()
                .as_ref()
                == placeholder,
            "Skeleton is not an unsigned canonical signing template"
        );
        let mut normalized = tx.clone();
        if kind != "ckb_multisig" {
            witness_lock(&mut normalized, indices[0], vec![0; placeholder.len()])?;
        }
        let mut h = gw_hash::blake2b::new_blake2b();
        h.update(&hash32(&tx.hash()?)?);
        for i in indices
            .iter()
            .copied()
            .chain(tx.inputs.len()..tx.witnesses.len())
        {
            let b = bytes(tx.witnesses.get(i).context("Missing witness")?)?;
            let b = if i == indices[0] {
                bytes(&normalized.witnesses[i])?
            } else {
                b
            };
            h.update(&(b.len() as u64).to_le_bytes());
            h.update(&b);
        }
        let mut message = [0; 32];
        h.finalize(&mut message);
        let digest: [u8; 32] = if kind != "ethereum_personal_sign" {
            message
        } else {
            Keccak256::digest([b"\x19Ethereum Signed Message:\n32".as_slice(), &message].concat())
                .into()
        };
        out.push(SigningGroup {
            lock_hash,
            input_indices: indices,
            kind: kind.into(),
            message: hx(message),
            signing_digest: hx(digest),
            required_signatures: required,
            require_first_n: first,
            identities,
        });
    }
    Ok(SigningRequest {
        transaction_hash: tx.hash()?,
        transaction_with_placeholders_hash: hx(hash(tx.packed()?.as_slice())),
        groups: out,
    })
}
pub(crate) fn recovered(group: &SigningGroup, signature: &str) -> Result<(usize, Vec<u8>)> {
    let mut b = bytes(signature)?;
    ensure!(b.len() == 65, "Signature must contain 65 bytes");
    if group.kind == "ethereum_personal_sign" && (27..=28).contains(&b[64]) {
        b[64] -= 27;
    }
    ensure!(b[64] <= 3, "Invalid recovery ID");
    let signature =
        RecoverableSignature::from_compact(&b[..64], RecoveryId::try_from(b[64] as i32)?)?;
    let key = Secp256k1::new().recover_ecdsa(
        &Message::from_digest(hash32(&group.signing_digest)?),
        &signature,
    )?;
    let id = if group.kind != "ethereum_personal_sign" {
        hx(&hash(&key.serialize())[..20])
    } else {
        hx(&Keccak256::digest(&key.serialize_uncompressed()[1..])[12..])
    };
    let position = group
        .identities
        .iter()
        .position(|v| v == &id)
        .context("Signature does not belong to this lock group")?;
    Ok((position, b))
}
/// Optional local offline signer. The caller reads a private key file; no secret enters the result.
pub fn sign(request: &SigningRequest, lock_hash: &str, secret: &[u8]) -> Result<Signatures> {
    let group = request
        .groups
        .iter()
        .find(|g| g.lock_hash == lock_hash)
        .context("Unknown signing group")?;
    let key = SecretKey::from_slice(secret)?;
    let signature = Secp256k1::new()
        .sign_ecdsa_recoverable(&Message::from_digest(hash32(&group.signing_digest)?), &key);
    let (id, bytes) = signature.serialize_compact();
    let signature = hx([bytes.as_slice(), &[i32::from(id) as u8]].concat());
    recovered(group, &signature)?;
    Ok(Signatures {
        transaction_hash: request.transaction_hash.clone(),
        signatures: vec![Signature {
            lock_hash: lock_hash.into(),
            signature,
        }],
    })
}
/// Published proposals contain complete Ops authorization and no user signatures.
pub fn verify_ops_signatures(request: &SigningRequest, responses: &[Signatures]) -> Result<()> {
    let mut found = BTreeMap::<String, BTreeSet<usize>>::new();
    for response in responses {
        ensure!(
            response.transaction_hash == request.transaction_hash,
            "Wrong signature transaction"
        );
        for sig in &response.signatures {
            let g = request
                .groups
                .iter()
                .find(|g| g.lock_hash == sig.lock_hash)
                .context("Unknown signature group")?;
            ensure!(
                g.kind != "ethereum_personal_sign",
                "Published proposal must not contain controller signatures"
            );
            let (i, _) = recovered(g, &sig.signature)?;
            ensure!(
                found.entry(sig.lock_hash.clone()).or_default().insert(i),
                "Duplicate signer"
            );
        }
    }
    for g in request
        .groups
        .iter()
        .filter(|g| g.kind != "ethereum_personal_sign")
    {
        let set = found
            .get(&g.lock_hash)
            .context("Missing Ops authorization")?;
        ensure!(
            set.len() == g.required_signatures && (0..g.require_first_n).all(|i| set.contains(&i)),
            "Incomplete Ops authorization"
        );
    }
    Ok(())
}
pub fn assemble(d: &Deployment, sk: &Skeleton, responses: &[Signatures]) -> Result<Transaction> {
    let request = requests(d, sk)?;
    let mut collected = BTreeMap::<String, BTreeMap<usize, Vec<u8>>>::new();
    for response in responses {
        ensure!(
            response.transaction_hash == request.transaction_hash,
            "Signatures belong to another transaction"
        );
        for sig in &response.signatures {
            let group = request
                .groups
                .iter()
                .find(|g| g.lock_hash == sig.lock_hash)
                .context("Unknown signature group")?;
            let (i, signature) = recovered(group, &sig.signature)?;
            ensure!(
                collected
                    .entry(sig.lock_hash.clone())
                    .or_default()
                    .insert(i, signature)
                    .is_none(),
                "Duplicate signer"
            );
        }
    }
    let mut tx = sk.transaction.clone();
    for group in request.groups {
        let sigs = collected
            .remove(&group.lock_hash)
            .context("Missing signature group")?;
        ensure!(
            sigs.len() == group.required_signatures
                && (0..group.require_first_n).all(|i| sigs.contains_key(&i)),
            "Signature threshold/require-first-n not satisfied"
        );
        let lock = if group.kind == "ckb_multisig" {
            let mut b = bytes(&d.multisig_config)?;
            for sig in sigs.values() {
                b.extend_from_slice(sig);
            }
            b
        } else if group.kind == "ckb_secp256k1" {
            sigs.values().next().context("No signature")?.clone()
        } else {
            omni(sigs.values().next().context("No signature")?)
        };
        witness_lock(&mut tx, group.input_indices[0], lock)?;
    }
    ensure!(
        tx.hash()? == request.transaction_hash,
        "Signing altered raw transaction"
    );
    Ok(tx)
}

/// Only explicit authority groups sign. PoA/helper scripts authorize via the producer inputs.
pub(crate) fn lock_template(
    d: &Deployment,
    p: &Profile,
    lock: &packed::Script,
) -> Result<Option<(&'static str, Vec<u8>, usize, usize, Vec<String>)>> {
    let gov = lock.as_slice() == p.base.governance.as_slice();
    let bootstrap = lock.as_slice() == p.base.bootstrap_lock.as_slice();
    let producer = match p {
        Profile::V0(v) => v
            .auxiliary
            .iter()
            .any(|a| a.before.type_.is_none() && a.before.lock.as_slice() == lock.as_slice()),
        _ => false,
    };
    let controller = c::control::ethereum_owner(lock).is_some();
    if !(gov || bootstrap || producer || controller) {
        return Ok(None);
    }
    let args = lock.args().raw_data();
    if p.protocol() == Protocol::V0 && bootstrap {
        return Ok(None);
    }
    if gov && p.protocol() == Protocol::V1 {
        let cfg = bytes(&d.multisig_config)?;
        let placeholder = d.multisig_placeholder(&p.base)?;
        return Ok(Some((
            "ckb_multisig",
            placeholder,
            cfg[2] as usize,
            cfg[1] as usize,
            cfg[4..].chunks_exact(20).map(hx).collect(),
        )));
    }
    ensure!(
        u8::from(lock.hash_type()) == 1,
        "Signing lock must use type hash"
    );
    if lock.code_hash().as_slice() == c::v0::SECP {
        ensure!(args.len() == 20, "Invalid standard secp args");
        return Ok(Some(("ckb_secp256k1", vec![0; 65], 1, 0, vec![hx(&args)])));
    }
    let old = hash32("0xa4398768d87bd17aea1361edc3accd6a0117774dc4ebc813bfa173e8ac0d086d")?;
    ensure!(
        lock.code_hash().as_slice() == c::OMNI || lock.code_hash().as_slice() == old,
        "Unsupported authority lock"
    );
    ensure!(
        args.len() == 22 && args[0] <= 1 && args[21] == 0,
        "Unsupported OmniLock identity/flags"
    );
    Ok(Some((
        if args[0] == 0 {
            "ckb_omnilock"
        } else {
            "ethereum_personal_sign"
        },
        omni(&[0; 65]),
        1,
        0,
        vec![hx(&args[1..21])],
    )))
}
