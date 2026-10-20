//! User-authorized L1 OmniLock spends. Independent of rollup governance and L2 mutations.
pub mod provenance;
pub mod server;
pub mod wallet;
use crate::{
    model::*,
    withdraw::{self, Dependency, Transaction},
};
use anyhow::{ensure, Context, Result};
use bech32::{FromBase32, ToBase32, Variant};
use exit_withdraw_state_validator as c;
use gw_types::{packed, prelude::*};
use secp256k1::{
    ecdsa::{RecoverableSignature, RecoveryId},
    Message, Secp256k1,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha3::{Digest, Keccak256};
use std::collections::{BTreeMap, BTreeSet};

pub const GENESIS: &str = "0x92b197aa1fba0f63633922c61c92375c9c074a93e85963554f5499fe1450d0e5";
pub const SUDT: &str = "0x5e7a36a77e68eecc013dfa2fe6a23f3b6c344b04005808694ae6dd45eea4cfd5";
pub const MAX_INPUTS: usize = 64;
pub const MAX_CYCLES: u64 = 70_000_000;
const CAP: u64 = 100_000_000;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Asset {
    pub asset_id: u32,
    pub symbol: Option<String>,
    pub decimals: Option<u8>,
    pub l1_decimals: Option<u8>,
    pub type_hash: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BuildRequest {
    pub address: String,
    pub destination: String,
    pub inputs: Vec<OutPoint>,
    /// None transfers all selected cells. Partial amounts use L1 units; CKB includes token capacity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub amounts: Option<Vec<ClaimAmount>>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClaimAmount {
    pub asset_id: u32,
    pub raw: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Quote {
    pub address: String,
    pub destination: String,
    pub destination_lock: Script,
    pub input_cells: Vec<Cell>,
    pub transaction: Transaction,
    pub transaction_hash: String,
    pub signing_message: String,
    pub fee_shannons: String,
    pub fee_rate: u64,
    pub received_capacity_shannons: String,
    pub minimum_capacity_shannons: String,
}

pub fn owner(address: &str) -> Result<Script> {
    let address = eth_address(address)?;
    Ok(Script {
        code_hash: hx(c::OMNI),
        hash_type: "type".into(),
        args: hx([&[1], address.as_slice(), &[0]].concat()),
    })
}
/// CKB RFC 0021: mainnet only, checksum variant must match the payload format.
pub fn parse_address(address: &str) -> Result<Script> {
    ensure!(
        address.len() <= 2048 && address.trim() == address,
        "Invalid CKB address length or whitespace"
    );
    let (hrp, data, variant) =
        bech32::decode(address).context("Invalid CKB address checksum/encoding")?;
    ensure!(
        hrp == "ckb",
        "Enter a CKB mainnet address (ckb), not a testnet or Ethereum address"
    );
    let b = Vec::<u8>::from_base32(&data)?;
    let format = *b.first().context("Empty address")?;
    let (code, hash_type, args) = match format {
        0 => {
            ensure!(
                variant == Variant::Bech32m && b.len() >= 34,
                "Invalid full CKB address"
            );
            let ty = match b[33] {
                0 => "data",
                1 => "type",
                2 => "data1",
                4 => "data2",
                _ => anyhow::bail!("Invalid CKB hash type"),
            };
            (hx(&b[1..33]), ty, hx(&b[34..]))
        }
        1 => {
            ensure!(
                variant == Variant::Bech32 && b.len() == 22,
                "Invalid short CKB address"
            );
            let code = match b[1] {
                0 => "0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8",
                1 => "0x5c5069eb0857efc65e1bca0c07df34c31663b3622fd3876c876320fc9634e2a8",
                2 => "0xd369597ff47f29fbc0d47d2e3775370d1250b85140c670e4718af712983a2354",
                _ => anyhow::bail!("Unknown short-address code index"),
            };
            (code.into(), "type", hx(&b[2..]))
        }
        2 | 4 => {
            ensure!(
                variant == Variant::Bech32 && b.len() >= 33,
                "Invalid legacy full CKB address"
            );
            (
                hx(&b[1..33]),
                if format == 2 { "data" } else { "type" },
                hx(&b[33..]),
            )
        }
        _ => anyhow::bail!("Unknown CKB address format"),
    };
    let script = Script {
        code_hash: code,
        hash_type: hash_type.into(),
        args,
    };
    script.packed()?;
    Ok(script)
}
pub fn address(script: &Script) -> Result<String> {
    let ty = match script.hash_type.as_str() {
        "data" => 0,
        "type" => 1,
        "data1" => 2,
        "data2" => 4,
        _ => anyhow::bail!("Invalid hash type"),
    };
    let mut b = vec![0];
    b.extend_from_slice(&hash32(&script.code_hash)?);
    b.push(ty);
    b.extend_from_slice(&bytes(&script.args)?);
    Ok(bech32::encode("ckb", b.to_base32(), Variant::Bech32m)?)
}
pub fn supported(cell: &Cell, assets: &[Asset]) -> Result<bool> {
    match &cell.output.type_script {
        None => Ok(cell.output_data == "0x"),
        Some(t) => Ok(t.code_hash == SUDT
            && t.hash_type == "type"
            && bytes(&t.args)?.len() == 32
            && bytes(&cell.output_data)?.len() == 16
            && assets
                .iter()
                .any(|a| a.asset_id != 1 && t.hash().ok().as_ref() == Some(&a.type_hash))),
    }
}
pub fn occupied(output: &CellOutput, data: &str) -> Result<u64> {
    let n = 8
        + 33
        + bytes(&output.lock.args)?.len()
        + match &output.type_script {
            Some(t) => 33 + bytes(&t.args)?.len(),
            None => 0,
        }
        + bytes(data)?.len();
    (n as u64).checked_mul(CAP).context("Capacity overflow")
}
fn witness(signature: &[u8]) -> String {
    let total = 20 + signature.len();
    let mut b = Vec::with_capacity(total);
    for n in [total, 16, total, total, signature.len()] {
        b.extend_from_slice(&(n as u32).to_le_bytes());
    }
    b.extend_from_slice(signature);
    hx(packed::WitnessArgs::new_builder()
        .lock(Some(gw_types::bytes::Bytes::from(b)).pack())
        .build()
        .as_slice())
}
pub fn signing_message(tx: &Transaction) -> Result<[u8; 32]> {
    ensure!(
        !tx.inputs.is_empty() && tx.witnesses.len() == tx.inputs.len(),
        "Invalid single-group witnesses"
    );
    let w = packed::WitnessArgs::from_slice(&bytes(&tx.witnesses[0])?)?;
    let zero = w
        .as_builder()
        .lock(Some(gw_types::bytes::Bytes::from(vec![0; 85])).pack())
        .build();
    let mut h = gw_hash::blake2b::new_blake2b();
    h.update(&hash32(&tx.hash()?)?);
    for i in 0..tx.witnesses.len() {
        let b = if i == 0 {
            zero.as_slice().to_vec()
        } else {
            bytes(&tx.witnesses[i])?
        };
        h.update(&(b.len() as u64).to_le_bytes());
        h.update(&b);
    }
    let mut digest = [0; 32];
    h.finalize(&mut digest);
    Ok(digest)
}
pub fn personal_digest(message: [u8; 32]) -> [u8; 32] {
    let mut h = Keccak256::new();
    h.update(b"\x19Ethereum Signed Message:\n32");
    h.update(message);
    h.finalize().into()
}
/// Sends every selected asset and all selected capacity (minus the exact fee) to one destination.
/// Never accepts a caller-provided transaction or arbitrary output list.
pub fn construct(
    request: &BuildRequest,
    inputs: Vec<Cell>,
    assets: &[Asset],
    deps: &[Dependency],
    fee_rate: u64,
) -> Result<Quote> {
    ensure!((1..=100_000).contains(&fee_rate), "Invalid fee rate");
    ensure!(
        !inputs.is_empty() && inputs.len() <= MAX_INPUTS && inputs.len() == request.inputs.len(),
        "Select between 1 and 64 cells"
    );
    let lock = owner(&request.address)?;
    let destination = parse_address(&request.destination)?;
    ensure!(
        destination != lock,
        "Destination must differ from this OmniLock wallet"
    );
    let mut seen = BTreeSet::new();
    let mut capacity = 0u64;
    let mut tokens = BTreeMap::<String, (Script, u128)>::new();
    for (cell, point) in inputs.iter().zip(&request.inputs) {
        ensure!(
            cell.out_point == *point && seen.insert(point.clone()),
            "Duplicate/mismatched input"
        );
        ensure!(
            cell.output.lock == lock,
            "Input belongs to a different ETH address"
        );
        ensure!(
            supported(cell, assets)?,
            "Unknown token or nonstandard cell data"
        );
        let n = quantity(&cell.output.capacity)?;
        ensure!(
            n >= occupied(&cell.output, &cell.output_data)?,
            "Invalid input occupied capacity"
        );
        capacity = capacity.checked_add(n).context("Capacity overflow")?;
        if let Some(t) = &cell.output.type_script {
            let b = bytes(&cell.output_data)?;
            let amount = u128::from_le_bytes(b.try_into().unwrap());
            let entry = tokens.entry(t.hash()?).or_insert((t.clone(), 0));
            entry.1 = entry
                .1
                .checked_add(amount)
                .context("Token total exceeds sUDT u128; select fewer cells")?;
        }
    }
    let mut tx = Transaction {
        version: "0x0".into(),
        cell_deps: deps
            .iter()
            .map(|d| json!({"out_point":d.out_point,"dep_type":d.dep_type}))
            .collect(),
        header_deps: vec![],
        inputs: request
            .inputs
            .iter()
            .map(|p| json!({"since":"0x0","previous_output":p}))
            .collect(),
        outputs: vec![],
        outputs_data: vec![],
        witnesses: vec!["0x".into(); inputs.len()],
    };
    tx.witnesses[0] = witness(&[0; 65]);
    let mut selected = BTreeMap::new();
    if let Some(amounts) = &request.amounts {
        ensure!(
            !amounts.is_empty() && amounts.len() <= assets.len(),
            "Invalid partial asset list"
        );
        for a in amounts {
            ensure!(
                !a.raw.is_empty() && a.raw.bytes().all(|b| b.is_ascii_digit()),
                "Amounts must be unsigned integer L1 units"
            );
            let n: u128 = a.raw.parse().context("Amount overflow")?;
            ensure!(
                selected.insert(a.asset_id, n).is_none(),
                "Duplicate selected asset"
            );
            let asset = assets
                .iter()
                .find(|x| x.asset_id == a.asset_id)
                .context("Unknown selected asset")?;
            let available = if a.asset_id == 1 {
                capacity as u128
            } else {
                tokens.get(&asset.type_hash).map(|(_, n)| *n).unwrap_or(0)
            };
            ensure!(
                n <= available,
                "Requested asset {} exceeds remaining balance",
                a.asset_id
            );
        }
        ensure!(
            selected.get(&1).copied().unwrap_or(0) > 0,
            "Specify destination CKB including token-cell capacity"
        );
    }
    let partial = request.amounts.is_some();
    // Recipient outputs first; change is always locked to the exact original ETH OmniLock.
    let mut change = Vec::new();
    for (hash, (type_script, amount)) in tokens {
        let take = if partial {
            let id = assets
                .iter()
                .find(|a| a.type_hash == hash)
                .context("Unknown token")?
                .asset_id;
            selected.get(&id).copied().unwrap_or(0)
        } else {
            amount
        };
        if take > 0 || !partial {
            tx.outputs.push(CellOutput {
                capacity: "0x0".into(),
                lock: destination.clone(),
                type_script: Some(type_script.clone()),
            });
            tx.outputs_data.push(hx(take.to_le_bytes()));
        }
        if amount > take || (partial && amount == 0) {
            change.push((type_script, amount - take));
        }
    }
    if tx.outputs.is_empty() {
        tx.outputs.push(CellOutput {
            capacity: "0x0".into(),
            lock: destination.clone(),
            type_script: None,
        });
        tx.outputs_data.push("0x".into());
    }
    let recipient_count = tx.outputs.len();
    if partial {
        for (type_script, n) in change {
            tx.outputs.push(CellOutput {
                capacity: "0x0".into(),
                lock: lock.clone(),
                type_script: Some(type_script),
            });
            tx.outputs_data.push(hx(n.to_le_bytes()));
        }
        if tx.outputs.len() == recipient_count {
            tx.outputs.push(CellOutput {
                capacity: "0x0".into(),
                lock,
                type_script: None,
            });
            tx.outputs_data.push("0x".into());
        }
    }
    let mut minimum = 0u64;
    let mut change_minimum = 0u64;
    for (i, (out, data)) in tx.outputs.iter_mut().zip(&tx.outputs_data).enumerate() {
        let n = occupied(out, data)?;
        out.capacity = format!("0x{n:x}");
        let total = if i < recipient_count {
            &mut minimum
        } else {
            &mut change_minimum
        };
        *total = total.checked_add(n).context("Capacity overflow")?;
    }
    let size = tx.packed()?.as_slice().len() as u64 + 4;
    ensure!(size <= 512_000, "Transaction size limit exceeded");
    let fee = size
        .checked_mul(fee_rate)
        .and_then(|x| x.checked_add(999))
        .context("Fee overflow")?
        / 1000;
    let spendable = capacity
        .checked_sub(fee)
        .context("Insufficient CKB for network fee")?;
    let received = if partial {
        u64::try_from(selected[&1]).context("CKB amount overflow")?
    } else {
        spendable
    };
    ensure!(
        received >= minimum,
        "Insufficient destination CKB: need at least {} shannons for selected assets",
        minimum
    );
    let remaining = spendable
        .checked_sub(received)
        .context("Requested CKB plus network fee exceeds remaining balance")?;
    ensure!(remaining >= change_minimum, "Insufficient CKB for change: need {} shannons after recipient amount and fee; reduce withdrawal or use transfer all", change_minimum);
    tx.outputs[0].capacity = format!(
        "0x{:x}",
        quantity(&tx.outputs[0].capacity)? + received - minimum
    );
    if partial {
        tx.outputs[recipient_count].capacity = format!(
            "0x{:x}",
            quantity(&tx.outputs[recipient_count].capacity)? + remaining - change_minimum
        );
    }
    Ok(Quote {
        address: hx(eth_address(&request.address)?),
        destination: request.destination.clone(),
        destination_lock: destination,
        transaction_hash: tx.hash()?,
        signing_message: hx(signing_message(&tx)?),
        transaction: tx,
        input_cells: inputs,
        fee_shannons: fee.to_string(),
        fee_rate,
        received_capacity_shannons: received.to_string(),
        minimum_capacity_shannons: minimum.to_string(),
    })
}
pub fn assemble(quote: &Quote, signature: &str) -> Result<Transaction> {
    let mut sig = bytes(signature)?;
    ensure!(sig.len() == 65, "Wallet signature must be 65 bytes");
    if sig[64] == 27 || sig[64] == 28 {
        sig[64] -= 27;
    }
    ensure!(sig[64] <= 1, "Unsupported Ethereum recovery ID");
    let recoverable =
        RecoverableSignature::from_compact(&sig[..64], RecoveryId::try_from(sig[64] as i32)?)?;
    let original = recoverable.to_standard();
    let mut normalized = original;
    normalized.normalize_s();
    ensure!(normalized == original, "Noncanonical high-s signature");
    let message = signing_message(&quote.transaction)?;
    ensure!(
        hx(message) == quote.signing_message && quote.transaction.hash()? == quote.transaction_hash,
        "Quote digest mismatch"
    );
    let key = Secp256k1::new().recover_ecdsa(
        &Message::from_digest(personal_digest(message)),
        &recoverable,
    )?;
    let digest = Keccak256::digest(&key.serialize_uncompressed()[1..]);
    ensure!(
        &digest[12..] == eth_address(&quote.address)?,
        "Signature does not match the connected ETH address"
    );
    let mut tx = quote.transaction.clone();
    tx.witnesses[0] = witness(&sig);
    Ok(tx)
}
