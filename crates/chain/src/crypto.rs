use anyhow::{anyhow, Error, Result};
use gw_common::blake2b::new_blake2b;
use lazy_static::lazy_static;
use secp256k1::recovery::{RecoverableSignature, RecoveryId};
use std::convert::{TryFrom, TryInto};

lazy_static! {
    pub static ref SECP256K1: secp256k1::Secp256k1<secp256k1::All> = secp256k1::Secp256k1::new();
}

pub type Message = [u8; 32];
pub type PubkeyHash = [u8; 20];
pub struct Signature(pub [u8; 65]);

impl TryFrom<&Signature> for RecoverableSignature {
    type Error = Error;

    fn try_from(sig: &Signature) -> Result<Self> {
        let recid = RecoveryId::from_i32(sig.0[64] as i32)?;
        let data = &sig.0[..64];
        let recoverable_sig = RecoverableSignature::from_compact(data, recid)?;
        Ok(recoverable_sig)
    }
}

pub fn verify_signature(
    sig: &Signature,
    msg: &Message,
    expected_pubkey_hash: &PubkeyHash,
) -> Result<()> {
    let msg = secp256k1::Message::from_slice(msg)?;
    let sig: RecoverableSignature = sig.try_into()?;
    let pubkey = SECP256K1.recover(&msg, &sig)?;
    let pubkey_hash = {
        let mut buf = [0u8; 32];
        let mut hasher = new_blake2b();
        hasher.update(&pubkey.serialize());
        hasher.finalize(&mut buf);
        let mut pubkey_hash = [0u8; 20];
        pubkey_hash.copy_from_slice(&buf[..20]);
        pubkey_hash
    };
    if &pubkey_hash != expected_pubkey_hash {
        return Err(anyhow!("wrong signature"));
    }
    Ok(())
}
