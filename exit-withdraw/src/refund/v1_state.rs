//! V1 receipt discovery and exact-runtime LP/farm facts. No historical table inputs.
use super::state::{word, Mapping, Reader};
use crate::{
    db::Database,
    model::{eth_address, hx},
    schema::COLUMN_TRANSACTION_RECEIPT,
};
use anyhow::{ensure, Context, Result};
use gw_common::state::State;
use gw_types::prelude::Reader as MoleculeReader;
use gw_types::{packed, prelude::*, registry_address::RegistryAddress};
use num_bigint::BigUint as B;
use serde_json::{json, Value};
use sha3::{Digest, Keccak256};
use std::collections::{BTreeMap, BTreeSet};
const PAIR: &str = "0xf601f6484fc84f0515c0ed834594c582a3974f00e4dc030ec489e8b57c75f8da";
const FACTORY: &str = "0x4ec3481f45a946241977d5338439efea01e7feff349aa56fbc78abcf8b0e3d36";
fn address(w: &[u8; 32]) -> Result<[u8; 20]> {
    ensure!(w[..12] == [0; 12], "Noncanonical storage address");
    Ok(w[12..].try_into()?)
}
fn le32(b: &[u8], start: usize) -> Result<usize> {
    Ok(u32::from_le_bytes(
        b.get(start..start + 4)
            .context("Truncated log integer")?
            .try_into()?,
    ) as usize)
}
fn registry<'a>(b: &'a [u8], offset: &mut usize) -> Result<Option<[u8; 20]>> {
    let len = le32(b, *offset + 4)?;
    ensure!(
        len == 0 || len == 20,
        "Unsupported native log address length"
    );
    let reg = RegistryAddress::from_slice(
        b.get(*offset..*offset + 8 + len)
            .context("Truncated registry log")?,
    )
    .context("Registry encoding")?;
    *offset += 8 + len;
    if len == 0 {
        return Ok(None);
    }
    ensure!(reg.registry_id == 2, "Non-ETH native history registry");
    Ok(Some(reg.address.as_slice().try_into()?))
}
fn transfer_addresses(b: &[u8], topic: &[u8; 32]) -> Result<Option<[[u8; 20]; 2]>> {
    ensure!(b.len() >= 28, "Truncated Polyjuice event");
    let data_len = le32(b, 20)?;
    let offset = 24usize
        .checked_add(data_len)
        .context("Event length overflow")?;
    let topics = le32(b, offset)?;
    ensure!(
        offset
            .checked_add(4)
            .and_then(|x| topics.checked_mul(32).and_then(|n| x.checked_add(n)))
            == Some(b.len()),
        "Event trailing or missing bytes"
    );
    if topics != 3 || data_len != 32 || b[offset + 4..offset + 36] != *topic {
        return Ok(None);
    }
    let from: [u8; 32] = b[offset + 36..offset + 68].try_into()?;
    let to: [u8; 32] = b[offset + 68..offset + 100].try_into()?;
    Ok(Some([address(&from)?, address(&to)?]))
}
/// History supplies candidates only. Every reported current balance is authenticated
/// independently against the frozen SMT. Event values never become refund amounts.
pub(super) fn history(db: &Database, accounts: &[Value]) -> Result<(BTreeSet<[u8; 20]>, Value)> {
    let known = accounts
        .iter()
        .filter_map(|a| a["address"].as_str())
        .map(eth_address)
        .collect::<Result<BTreeSet<_>>>()?;
    let native = accounts
        .iter()
        .filter(|a| a["kind"] == "native_sudt")
        .map(|a| a["account_id"].as_u64().unwrap() as u32)
        .collect::<BTreeSet<_>>();
    let mut candidates = BTreeSet::new();
    let mut native_candidates = BTreeSet::new();
    let topic: [u8; 32] = Keccak256::digest(b"Transfer(address,address,uint256)").into();
    let store = db.tree.store();
    let cf = store
        .db
        .cf_handle(&COLUMN_TRANSACTION_RECEIPT.to_string())
        .context("Missing receipt history")?;
    let mut opts = rocksdb::ReadOptions::default();
    opts.fill_cache(false);
    let mut count = 0u64;
    for item in store
        .db
        .iterator_cf_opt(cf, opts, rocksdb::IteratorMode::Start)
    {
        let (_, value) = item?;
        let receipt =
            packed::TxReceiptReader::from_slice(&value).context("Malformed V1 receipt")?;
        for log in receipt.logs().iter() {
            let flag = log.service_flag().as_slice()[0];
            let data = log.data().raw_data();
            if flag <= 1 {
                let asset: u32 = log.account_id().unpack();
                let mut offset = 0;
                let from = registry(data, &mut offset)?;
                let to = registry(data, &mut offset)?;
                ensure!(data.len() == offset + 32, "Native log amount length");
                ensure!(
                    native.contains(&asset),
                    "Native log from non-native account"
                );
                for owner in [from, to].into_iter().flatten() {
                    candidates.insert(owner);
                    if !known.contains(&owner) {
                        native_candidates.insert((asset, owner));
                    }
                }
            } else if flag == 3 {
                if let Some(owners) = transfer_addresses(data, &topic)? {
                    candidates.extend(owners);
                }
            }
        }
        count += 1;
        if count % 1_000_000 == 0 {
            eprintln!(
                "v1 receipt history {count}; discovered {} addresses",
                candidates.len()
            );
        }
    }
    ensure!(count > 0, "Empty required receipt history");
    let mut balances = vec![];
    for (asset, owner) in &native_candidates {
        let raw = db.get_sudt_balance(*asset, &RegistryAddress::new(2, owner.to_vec()))?;
        if !raw.is_zero() {
            balances.push(json!({"asset_account_id":asset,"address":hx(owner),"raw_amount":raw.to_string(),"disposition":"unregistered_owner_retained"}));
        }
    }
    Ok((
        candidates,
        json!({"receipts_scanned":count,"unknown_native_holders":balances,"unknown_native_candidates_checked":native_candidates.len(),"history_is_discovery_not_ownership_proof":true,"current_balances_authenticated":true}),
    ))
}
fn census(
    reader: &Reader,
    id: u32,
    mapping: &Mapping,
    candidates: &[[u8; 20]],
) -> Result<(B, Vec<Value>)> {
    let mut total = B::default();
    let mut holders = vec![];
    for chunk in candidates.chunks(256) {
        let keys = chunk
            .iter()
            .map(|a| mapping.keys(a))
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        let values = reader.storage(id, &keys)?;
        for (owner, fields) in chunk.iter().zip(values.chunks(mapping.width as usize)) {
            let amount = B::from_bytes_be(&fields[0]);
            if amount != B::default() {
                total += &amount;
                holders.push(json!({"address":hx(owner),"raw":amount.to_string(),"fields":fields.iter().map(hx).collect::<Vec<_>>()}));
            }
        }
    }
    Ok((total, holders))
}
fn mapping(slot: u64, width: u8, key_before: Option<String>) -> Mapping {
    Mapping {
        slot,
        width,
        key_before,
        slot_first: false,
        key_after: None,
    }
}
pub(super) fn extract(
    reader: &Reader,
    accounts: &[Value],
    candidates: &[[u8; 20]],
    history: Value,
) -> Result<Value> {
    ensure!(matches!(reader, Reader::V1(_)), "V1 facts require V1 DB");
    let by_address = accounts
        .iter()
        .filter_map(|a| a["address"].as_str().map(|s| (s, a)))
        .collect::<BTreeMap<_, _>>();
    let mut pairs = vec![];
    for a in accounts.iter().filter(|a| a["runtime_hash"] == PAIR) {
        let id = a["account_id"].as_u64().context("Pair id")? as u32;
        let slots = reader.storage(id, &(0..13).map(word).collect::<Vec<_>>())?;
        let supply = B::from_bytes_be(&slots[0]);
        if supply == B::default() {
            continue;
        }
        let factory = hx(address(&slots[5])?);
        let factory_account = by_address
            .get(factory.as_str())
            .context("Unknown pair factory")?;
        ensure!(
            factory_account["runtime_hash"] == FACTORY,
            "Unreviewed pair factory {id}"
        );
        let fee_to = address(&reader.value(
            factory_account["account_id"].as_u64().unwrap() as u32,
            word(0),
        )?)?;
        let r0 = B::from_bytes_be(&slots[8][18..]);
        let r1 = B::from_bytes_be(&slots[8][4..18]);
        let last = B::from_bytes_be(&slots[11]);
        let fee = crate::v0::yokai_fee(&supply, &r0, &r1, &last, fee_to != [0; 20]);
        let (sum, holders) = census(reader, id, &mapping(1, 1, None), candidates)?;
        ensure!(
            sum == supply,
            "Pair {id} holder closure failed: {sum} != {supply}"
        );
        pairs.push(json!({"account_id":id,"address":a["address"],"factory":factory,"fee_to":hx(fee_to),"token0":hx(address(&slots[6])?),"token1":hx(address(&slots[7])?),"reserve0":r0.to_string(),"reserve1":r1.to_string(),"total_supply":supply.to_string(),"k_last":last.to_string(),"fee_lp_to_mint":fee.to_string(),"effective_supply":(&supply+&fee).to_string(),"holders":holders,"holder_closure":true}));
        eprintln!("v1 pair {id}: holder supply verified");
    }
    let mut farms = vec![];
    for (id, hash) in [
        (
            255,
            "0x57c534048c7c14a789630720947a9b0af753cebc0a3cb48b0435832d42b2df3e",
        ),
        (
            265,
            "0x6455a62b2dd39519e3aa48ee8086bc1370c01089595e2e8551b758ba443dda51",
        ),
        (
            266,
            "0xd57fa2d0d7d5e516cc822720cda0a47c7d4a1b970eab4b3ae8e0b717e1bed8fa",
        ),
    ] {
        ensure!(
            accounts[id as usize]["account_id"] == id
                && accounts[id as usize]["runtime_hash"] == hash,
            "Farm runtime changed {id}"
        );
        let prefix = reader.storage(id, &(0..32).map(word).collect::<Vec<_>>())?;
        let count = if id == 266 {
            1
        } else {
            let n = B::from_bytes_be(&prefix[6]);
            ensure!(n < 100u8.into(), "Farm pool bound");
            n.to_u64_digits().first().copied().unwrap_or_default()
        };
        for pid in 0..count {
            let m = if id == 266 {
                mapping(1, 4, None)
            } else {
                mapping(7, 2, Some(hx(word(pid))))
            };
            let (sum, holders) = census(reader, id, &m, candidates)?;
            let pool_fields = if id == 266 {
                vec![]
            } else {
                let start = B::from_bytes_be(&Keccak256::digest(word(6))) + B::from(pid) * 5u8;
                let keys = (0..5u8)
                    .map(|i| {
                        let b = (&start + i).to_bytes_be();
                        ensure!(b.len() <= 32, "Pool offset");
                        let mut k = [0; 32];
                        k[32 - b.len()..].copy_from_slice(&b);
                        Ok(k)
                    })
                    .collect::<Result<Vec<_>>>()?;
                reader.storage(id, &keys)?
            };
            farms.push(json!({"account_id":id,"pool_id":pid,"principal_or_shares_raw":sum.to_string(),"users":holders,"pool_fields":pool_fields.iter().map(hx).collect::<Vec<_>>(),"linear_slots":prefix.iter().map(hx).collect::<Vec<_>>(),"layout":m}));
            eprintln!("v1 farm {id} pool {pid}: principal census complete");
        }
    }
    Ok(json!({"pairs":pairs,"farms":farms,"history":history,"complete_refund_calculation":false}))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn history_parser_rejects_truncation_and_noncanonical_addresses() {
        let topic: [u8; 32] = Keccak256::digest(b"Transfer(address,address,uint256)").into();
        let mut event = vec![0; 160];
        event[20..24].copy_from_slice(&32u32.to_le_bytes());
        event[56..60].copy_from_slice(&3u32.to_le_bytes());
        event[60..92].copy_from_slice(&topic);
        event.truncate(156);
        assert_eq!(
            transfer_addresses(&event, &topic).unwrap(),
            Some([[0; 20]; 2])
        );
        for len in 0..event.len() {
            assert!(transfer_addresses(&event[..len], &topic).is_err());
        }
        event[92] = 1;
        assert!(transfer_addresses(&event, &topic).is_err());
        assert!(registry(&[0; 7], &mut 0).is_err());
        let mut native = vec![0; 28];
        native[..4].copy_from_slice(&2u32.to_le_bytes());
        native[4..8].copy_from_slice(&20u32.to_le_bytes());
        assert_eq!(registry(&native, &mut 0).unwrap(), Some([0; 20]));
        native[4] = 21;
        assert!(registry(&native, &mut 0).is_err());
    }
}
