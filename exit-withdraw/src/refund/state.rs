//! Offline authenticated storage extraction for versioned refund adapters.
//! Recipes contain identities and keys only. They never contain monetary state.
use crate::{
    db::Database,
    model::{bytes, eth_address, hash32, hx},
    settlement::adapter::{NativeDb, Protocol},
    v0::{self, V0Db},
};
use anyhow::{ensure, Context, Result};
use num_bigint::BigUint;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sha3::Keccak256;
use std::{collections::BTreeSet, path::Path};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Recipes {
    schema: String,
    protocol: Protocol,
    root: String,
    accounts: Vec<AccountRecipe>,
    candidate_hints: Vec<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AccountRecipe {
    account_id: u32,
    address: String,
    script_hash: String,
    runtime_hash: String,
    prefix_words: u32,
    slots: Vec<String>,
    mappings: Vec<Mapping>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Mapping {
    pub(super) slot: u64,
    pub(super) width: u8,
    pub(super) slot_first: bool,
    pub(super) key_before: Option<String>,
    pub(super) key_after: Option<String>,
}
pub fn word(n: u64) -> [u8; 32] {
    let mut w = [0; 32];
    w[24..].copy_from_slice(&n.to_be_bytes());
    w
}
fn padded_key(s: &str) -> Result<[u8; 32]> {
    let v = bytes(s)?;
    ensure!(v.len() <= 32, "Mapping key wider than 32 bytes");
    let mut w = [0; 32];
    w[32 - v.len()..].copy_from_slice(&v);
    Ok(w)
}
fn hash_pair(a: &[u8; 32], b: &[u8; 32]) -> [u8; 32] {
    let mut input = [0; 64];
    input[..32].copy_from_slice(a);
    input[32..].copy_from_slice(b);
    Keccak256::digest(input).into()
}
impl Mapping {
    pub(super) fn keys(&self, address: &[u8; 20]) -> Result<Vec<[u8; 32]>> {
        ensure!((1..=8).contains(&self.width), "Mapping width outside 1..8");
        let mut base = word(self.slot);
        if let Some(k) = &self.key_before {
            base = hash_pair(&padded_key(k)?, &base);
        }
        let mut owner = [0; 32];
        owner[12..].copy_from_slice(address);
        base = if self.slot_first {
            hash_pair(&base, &owner)
        } else {
            hash_pair(&owner, &base)
        };
        if let Some(k) = &self.key_after {
            base = hash_pair(&padded_key(k)?, &base);
        }
        let mut keys = Vec::with_capacity(self.width.into());
        for off in 0..self.width {
            if off > 0 {
                let mut carry = true;
                for b in base.iter_mut().rev() {
                    let (n, c) = b.overflowing_add(1);
                    *b = n;
                    carry = c;
                    if !c {
                        break;
                    }
                }
                ensure!(!carry, "Mapping storage offset overflow");
            }
            keys.push(base);
        }
        Ok(keys)
    }
}

pub enum Reader {
    V0(V0Db),
    V1(Database),
}
impl Reader {
    pub fn open(protocol: Protocol, path: &Path) -> Result<Self> {
        match protocol {
            Protocol::V0 => {
                let mut db = V0Db::open(path)?;
                db.rebuild()?;
                ensure!(db.tip["account_root"] == db.root(), "V0 root/tip mismatch");
                Ok(Self::V0(db))
            }
            Protocol::V1 => {
                let db = Database::open(path)?;
                ensure!(db.tip.account_root == db.root(), "V1 root/tip mismatch");
                Ok(Self::V1(db))
            }
        }
    }
    pub fn root(&self) -> String {
        match self {
            Self::V0(db) => db.root(),
            Self::V1(db) => db.root(),
        }
    }
    pub fn count(&self) -> Result<u32> {
        match self {
            Self::V0(db) => Ok(u32::try_from(
                db.tip["account_count"].as_u64().context("account count")?,
            )?),
            Self::V1(db) => Ok(db.tip.account_count),
        }
    }
    pub fn storage(&self, id: u32, keys: &[[u8; 32]]) -> Result<Vec<[u8; 32]>> {
        ensure!(id < self.count()?, "Storage account outside snapshot");
        match self {
            Self::V0(db) => db.raw_many(&keys.iter().map(|k| v0::kv(id, k)).collect::<Vec<_>>()),
            Self::V1(db) => db.raw_many(
                &keys
                    .iter()
                    .map(|k| gw_common::state::build_account_key(id, k))
                    .collect::<Vec<_>>(),
            ),
        }
    }
    pub fn value(&self, id: u32, key: [u8; 32]) -> Result<[u8; 32]> {
        Ok(self.storage(id, &[key])?[0])
    }
    pub fn number(&self, id: u32, slot: u64) -> Result<BigUint> {
        Ok(BigUint::from_bytes_be(&self.value(id, word(slot))?))
    }
    pub fn address(&self, id: u32) -> Result<Option<[u8; 20]>> {
        match self {
            Self::V0(db) => {
                let (h, s) = NativeDb::account(db, id)?;
                ensure!(s.hash()? == hx(h), "V0 script preimage");
                Ok(Some(h[..20].try_into()?))
            }
            Self::V1(db) => {
                let (h, _) = db.account(id)?;
                db.registry(&h)?
                    .map(|r| {
                        ensure!(
                            r.registry_id == 2 && r.address.len() == 20,
                            "Unsupported registry identity"
                        );
                        Ok(r.address.as_slice().try_into()?)
                    })
                    .transpose()
            }
        }
    }
    pub fn accounts(&self) -> Result<Vec<Value>> {
        let mut out = Vec::with_capacity(self.count()? as usize);
        let native_hash = match self {
            Self::V0(_) => v0::SUDT.to_owned(),
            Self::V1(db) => db.account(1)?.1.code_hash,
        };
        for id in 0..self.count()? {
            let (h, script) = match self {
                Self::V0(db) => NativeDb::account(db, id)?,
                Self::V1(db) => db.account(id)?,
            };
            ensure!(script.hash()? == hx(h), "Account script preimage");
            let args = bytes(&script.args)?;
            let (eth, poly, sudt, rollup) = match self {
                Self::V0(_) => (v0::ETH, v0::POLY, v0::SUDT, v0::ROLLUP),
                Self::V1(_) => (
                    "0x096df264f38fff07f3acd318995abc2c71ae0e504036fe32bc38d5b6037364d4",
                    "0x83d5d8841518e8db686909d27c821398491f475ed5f1cd392c36e83f4252c4ac",
                    native_hash.as_str(),
                    crate::model::ROLLUP_HASH,
                ),
            };
            ensure!(args.starts_with(&hash32(rollup)?), "Foreign account prefix");
            let kind = if script.hash_type != "type" {
                "unknown"
            } else if script.code_hash == eth && args.len() == 52 {
                "eth_eoa"
            } else if script.code_hash == poly {
                "polyjuice"
            } else if script.code_hash == sudt && args.len() == 64 {
                "native_sudt"
            } else {
                "other_system_or_unknown"
            };
            let owner = if kind == "eth_eoa" {
                Some(hx(&args[32..]))
            } else {
                None
            };
            let address = self.address(id)?.map(hx);
            let mut row = json!({"account_id":id,"script_hash":hx(h),"script":script,"address":address,"l2_short_address":if matches!(self,Self::V0(_)){Some(hx(&h[..20]))}else{None},"owner_eth_address":owner,"kind":kind});
            if kind == "polyjuice" {
                let mut k = v0::field(id, 255);
                k[5] = 1;
                row["runtime_hash"] = json!(hx(self.value(id, k)?));
                k[5] = 2;
                row["destructed_flag"] = json!(hx(self.value(id, k)?));
            }
            out.push(row);
        }
        Ok(out)
    }
    fn validate_recipe(&self, r: &AccountRecipe) -> Result<()> {
        let (h, s) = match self {
            Self::V0(db) => NativeDb::account(db, r.account_id)?,
            Self::V1(db) => db.account(r.account_id)?,
        };
        ensure!(
            hx(h) == r.script_hash && s.hash()? == r.script_hash,
            "Recipe script identity mismatch for {}",
            r.account_id
        );
        ensure!(
            self.address(r.account_id)? == Some(eth_address(&r.address)?),
            "Recipe address mismatch for {}",
            r.account_id
        );
        let mut key = v0::field(r.account_id, 255);
        key[5] = 1;
        ensure!(
            self.value(r.account_id, key)? == hash32(&r.runtime_hash)?,
            "Recipe runtime mismatch for {}",
            r.account_id
        );
        Ok(())
    }
}
fn embedded(protocol: Protocol) -> &'static [u8] {
    match protocol {
        Protocol::V0 => include_bytes!("policy/v0-storage.json"),
        Protocol::V1 => include_bytes!("policy/v1-storage.json"),
    }
}

/// Extract values anew, without opening old tables, snapshots, RPC results or layout files.
/// This produces raw protocol facts, not a complete beneficiary allocation.
pub fn extract(protocol: Protocol, path: &Path) -> Result<Value> {
    let src = embedded(protocol);
    let recipes: Recipes = serde_json::from_slice(src)?;
    ensure!(
        recipes.protocol == protocol && recipes.schema == "refund-storage-recipes-1",
        "Recipe version"
    );
    let reader = Reader::open(protocol, path)?;
    ensure!(
        reader.root() == recipes.root,
        "Unsupported refund snapshot root"
    );
    let mut candidates = BTreeSet::from([[0; 20]]);
    candidates.insert(eth_address("0x000000000000000000000000000000000000dead")?);
    let accounts = reader.accounts()?;
    let authorities = if protocol == Protocol::V0 {
        super::authority::v0(&reader, &accounts)?
    } else {
        Value::Null
    };
    for account in &accounts {
        if let Some(address) = account["address"].as_str() {
            candidates.insert(eth_address(address)?);
        }
    }
    for hint in &recipes.candidate_hints {
        candidates.insert(eth_address(hint)?);
    }
    let history = if let Reader::V1(db) = &reader {
        let (discovered, report) = super::v1_state::history(db, &accounts)?;
        candidates.extend(discovered);
        report
    } else {
        Value::Null
    };
    let candidates: Vec<_> = candidates.into_iter().collect();
    let mut rows = Vec::with_capacity(recipes.accounts.len());
    let mut reads = 0usize;
    // Validate every identity before the first potentially expensive holder scan.
    for r in &recipes.accounts {
        reader.validate_recipe(r)?;
    }
    for r in &recipes.accounts {
        ensure!(r.prefix_words <= 10000, "Prefix read bound");
        let mut keys = (0..r.prefix_words)
            .map(|i| word(i.into()))
            .collect::<Vec<_>>();
        keys.extend(
            r.slots
                .iter()
                .map(|s| hash32(s))
                .collect::<Result<Vec<_>>>()?,
        );
        keys.sort();
        ensure!(
            keys.windows(2).all(|w| w[0] != w[1]),
            "Duplicate recipe storage key"
        );
        let values = reader.storage(r.account_id, &keys)?;
        reads += keys.len();
        let slots: Vec<_> = keys
            .iter()
            .zip(values)
            .map(|(k, v)| json!({"key":hx(k),"value":hx(v)}))
            .collect();
        let mut mappings = Vec::with_capacity(r.mappings.len());
        for m in &r.mappings {
            let mut nonzero = vec![];
            for chunk in candidates.chunks(256) {
                let keys = chunk
                    .iter()
                    .map(|a| m.keys(a))
                    .collect::<Result<Vec<_>>>()?
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>();
                let values = reader.storage(r.account_id, &keys)?;
                reads += keys.len();
                for (address, words) in chunk.iter().zip(values.chunks(m.width.into())) {
                    if words.iter().any(|w| *w != [0; 32]) {
                        nonzero.push(json!({"address":hx(address),"fields":words.iter().map(hx).collect::<Vec<_>>()}));
                    }
                }
            }
            mappings.push(json!({"layout":m,"nonzero_records":nonzero}));
        }
        eprintln!(
            "{} protocol storage {}: {} authenticated words",
            protocol.name(),
            r.account_id,
            reads
        );
        rows.push(json!({"account_id":r.account_id,"address":r.address,"script_hash":r.script_hash,"runtime_hash":r.runtime_hash,"slots":slots,"mappings":mappings}));
    }
    let native_protocols = if let Reader::V0(db) = &reader {
        json!({"pairs":v0::yokai_pairs_from_db(db,&accounts)?,"wckb":v0::wckb_holders_from_db(db,&accounts)?,"farms":v0::farms_from_db(db,&accounts)?,"history":v0::beneficiary_history_from_db(db,&accounts)?,"authorities":authorities})
    } else {
        {
        let mut protocols=super::v1_state::extract(&reader, &accounts, &candidates, history)?;
        protocols["receipt_ledgers"]=super::v1_ledgers::extract(&reader,&accounts,&rows)?;
        protocols
        }
    };
    Ok(
        json!({"schema":"refund-protocol-state-1","protocol":protocol,"root":reader.root(),"recipe_sha256":hex::encode(Sha256::digest(src)),"candidate_count":candidates.len(),"storage_word_reads":reads,"rows":rows,"accounts":accounts,"native_protocols":native_protocols,"complete_refund_calculation":false,"limitations":["Authenticated raw storage, not inferred economic rights. Economic adapters must select reviewed layouts and enforce supply, source and ownership invariants.","Mapping candidates comprise current accounts, zero/dead and embedded address discovery hints. Their values are read from DB; hints do not establish holder completeness. History discovery and per-protocol completeness checks remain required."]}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn embedded_recipes_are_strict_and_have_no_monetary_fields() {
        for protocol in [Protocol::V0, Protocol::V1] {
            let r: Recipes = serde_json::from_slice(embedded(protocol)).unwrap();
            assert_eq!(r.protocol, protocol);
            assert!(!r.accounts.is_empty());
            let mut ids = BTreeSet::new();
            for a in r.accounts {
                assert!(ids.insert(a.account_id));
                hash32(&a.script_hash).unwrap();
                hash32(&a.runtime_hash).unwrap();
                eth_address(&a.address).unwrap();
                for m in a.mappings {
                    m.keys(&[0; 20]).unwrap();
                }
            }
            let mut value: Value = serde_json::from_slice(embedded(protocol)).unwrap();
            value["accounts"][0]["balance_raw"] = json!("999");
            assert!(serde_json::from_value::<Recipes>(value).is_err());
        }
    }
    #[test]
    fn solidity_vyper_nested_and_struct_slots_are_distinct() {
        let addr = [7; 20];
        let mut owner = [0; 32];
        owner[12..].copy_from_slice(&addr);
        let m = Mapping {
            slot: 3,
            width: 2,
            slot_first: false,
            key_before: None,
            key_after: None,
        };
        let keys = m.keys(&addr).unwrap();
        assert_eq!(keys[0], hash_pair(&owner, &word(3)));
        assert_eq!(
            BigUint::from_bytes_be(&keys[1]),
            BigUint::from_bytes_be(&keys[0]) + 1u8
        );
        let mut v = m.clone();
        v.slot_first = true;
        assert_eq!(v.keys(&addr).unwrap()[0], hash_pair(&word(3), &owner));
        assert_ne!(v.keys(&addr).unwrap(), keys);
        v = m.clone();
        v.key_before = Some(hx(word(9)));
        assert_eq!(
            v.keys(&addr).unwrap()[0],
            hash_pair(&owner, &hash_pair(&word(9), &word(3)))
        );
        v = m;
        v.key_after = Some(hx(word(9)));
        assert_eq!(v.keys(&addr).unwrap()[0], hash_pair(&word(9), &keys[0]));
        v.width = 0;
        assert!(v.keys(&addr).is_err());
        v.width = 1;
        v.key_after = Some(hx([0; 33]));
        assert!(v.keys(&addr).is_err());
    }
}
