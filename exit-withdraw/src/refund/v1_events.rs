//! Canonical-DB receipt evidence for Balancer registration/internal balances and
//! reviewed lending treasury provenance. Events never substitute for current SMT balances.
use super::state::Reader;
use crate::{
    model::{eth_address, hash, hx},
    schema::{COLUMN_BLOCK, COLUMN_INDEX, COLUMN_TRANSACTION_RECEIPT},
};
use anyhow::{ensure, Context, Result};
use gw_types::{packed, prelude::Reader as MoleculeReader};
use serde_json::{json, Value};
use sha3::{Digest, Keccak256};
fn u32le(b: &[u8], i: usize) -> Result<usize> {
    Ok(u32::from_le_bytes(
        b.get(i..i + 4)
            .context("Truncated event integer")?
            .try_into()?,
    ) as usize)
}
fn parts(b: &[u8]) -> Result<(&[u8], &[u8], &[u8])> {
    ensure!(b.len() >= 28, "Truncated event");
    let len = u32le(b, 20)?;
    let end = 24usize.checked_add(len).context("Event length overflow")?;
    let count = u32le(b, end)?;
    ensure!(
        end.checked_add(4)
            .and_then(|x| count.checked_mul(32).and_then(|n| x.checked_add(n)))
            == Some(b.len()),
        "Event topics length"
    );
    Ok((&b[..20], &b[24..end], &b[end + 4..]))
}
pub fn extract(reader: &Reader) -> Result<Value> {
    let Reader::V1(db) = reader else {
        anyhow::bail!("V1 events require V1 DB")
    };
    let store = db.tree.store();
    let cf = store
        .db
        .cf_handle(&COLUMN_TRANSACTION_RECEIPT.to_string())
        .context("Missing receipts")?;
    let signatures = [
        "PoolRegistered(bytes32,address,uint8)",
        "InternalBalanceChanged(address,address,int256)",
        "PoolBalanceManaged(bytes32,address,address,int256,int256)",
        "Deposit(address,address,address,uint256,uint16)",
    ];
    let topics = signatures
        .iter()
        .map(|s| <[u8; 32]>::from(Keccak256::digest(s.as_bytes())))
        .collect::<Vec<_>>();
    let transfer: [u8; 32] = Keccak256::digest(b"Transfer(address,address,uint256)").into();
    let treasury = [
        eth_address("0x3c98274aff14ee0deb7d230da173b1bcfc14fd48")?,
        eth_address("0xb8832fbf0f2438efd9fa90d2e17c0454a2daa942")?,
    ];
    let mut opts = rocksdb::ReadOptions::default();
    opts.fill_cache(false);
    let mut count = 0u64;
    let mut events = vec![];
    let mut ignored_noncanonical = 0u64;
    for item in store
        .db
        .iterator_cf_opt(cf, opts, rocksdb::IteratorMode::Start)
    {
        let (key, value) = item?;
        let receipt = packed::TxReceiptReader::from_slice(&value).context("Malformed receipt")?;
        let mut selected = vec![];
        for (index, log) in receipt.logs().iter().enumerate() {
            if log.service_flag().as_slice()[0] != 3 {
                continue;
            }
            let (emitter, data, ts) = parts(log.data().raw_data())?;
            if ts.len() < 32 {
                continue;
            }
            let first: &[u8; 32] = ts[..32].try_into()?;
            let treasury_transfer = *first == transfer
                && ts.len() == 96
                && treasury
                    .iter()
                    .any(|a| ts[76..96] == *a || ts[44..64] == *a);
            if topics.contains(first) || treasury_transfer {
                selected.push(json!({"emitter":hx(emitter),"data":hx(data),"topics":ts.chunks(32).map(hx).collect::<Vec<_>>(),"log_index":index}));
            }
        }
        count += 1;
        if count % 1_000_000 == 0 {
            eprintln!("v1 policy receipt evidence {count}");
        }
        if selected.is_empty() {
            continue;
        }
        ensure!(key.len() == 36, "Receipt key encoding");
        let hash_key = &key[..32];
        let tx_index = u32::from_be_bytes(key[32..].try_into()?) as usize;
        let Some(number) = store.get(COLUMN_INDEX, hash_key)? else {
            ignored_noncanonical += 1;
            continue;
        };
        ensure!(number.len() == 8, "Block index number width");
        let number = u64::from_le_bytes(number.as_slice().try_into()?);
        if number > db.tip.number {
            ignored_noncanonical += 1;
            continue;
        }
        ensure!(
            store.get(COLUMN_INDEX, &number.to_le_bytes())?.as_deref() == Some(hash_key),
            "Conflicting canonical index"
        );
        let block = store
            .get(COLUMN_BLOCK, hash_key)?
            .context("Missing event block")?;
        let block = packed::L2BlockReader::from_slice(&block).context("Event block encoding")?;
        ensure!(
            hash(block.raw().as_slice()).as_slice() == hash_key,
            "Event block hash mismatch"
        );
        let tx = block
            .transactions()
            .get(tx_index)
            .context("Event transaction index")?;
        ensure!(
            hash(tx.as_slice()).as_slice() == receipt.tx_witness_hash().as_slice(),
            "Receipt transaction witness mismatch"
        );
        for mut event in selected {
            event["block_number"] = json!(number);
            event["block_hash"] = json!(hx(hash_key));
            event["transaction_index"] = json!(tx_index);
            events.push(event);
        }
    }
    ensure!(count > 0, "Empty receipt history");
    Ok(
        json!({"root":reader.root(),"receipts_scanned":count,"ignored_noncanonical_receipts_with_selected_events":ignored_noncanonical,"events":events,"current_balances_must_be_authenticated_separately":true,"limitation":"Receipt event payloads are database historical evidence, not an independent SMT commitment or transaction reexecution proof."}),
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn malformed_event_lengths_rejected() {
        assert!(parts(&[0; 27]).is_err());
        let mut a = vec![0; 28];
        assert!(parts(&a).is_ok());
        a[20] = 255;
        assert!(parts(&a).is_err());
    }
}
