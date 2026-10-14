//! Read-only v0 adapter. No registry-address or native total-supply assumptions.
use crate::{db::ReadStore, model::*, network::Rpc};
use anyhow::{ensure, Context, Result};
use gw_smt::{
    smt::{Blake2bHasher, SMT},
    sparse_merkle_tree::{traits::StoreReadOps, H256},
};
use rocksdb::{Options, DB};
use serde_json::{json, Value};
use std::{collections::BTreeMap, fs, path::Path, sync::Arc};
pub const ROLLUP: &str = "0x40d73f0d3c561fcaae330eabc030d8d96a9d0af36d0c5114883658a350cb9e3b";
pub const ETH: &str = "0x1563080d175bf8ddd44a48e850cecf0c0b4575835756eb5ffd53ad830931b9f9";
pub const TRON: &str = "0x0bc55f318d738bd1f50e5d950110f0ce5ea2230ec1b62defe1271e06e680476f";
pub const SUDT: &str = "0xdb9896ecb952ab72f4f533d33fd9562fc1333fb6903899e93921efa9f8791408";
pub const POLY: &str = "0x636b89329db092883883ab5256e435ccabeee07b52091a78be22179636affce8";
fn u32le(b: &[u8]) -> Result<u32> {
    Ok(u32::from_le_bytes(b.try_into()?))
}
fn u64le(b: &[u8]) -> Result<u64> {
    Ok(u64::from_le_bytes(b.try_into()?))
}
/// Exact Molecule table parser for pinned historical schema boundaries.
pub fn table(b: &[u8], count: usize) -> Result<Vec<&[u8]>> {
    ensure!(count > 0, "Empty table schema is unsupported");
    ensure!(
        b.len() >= 4 + count * 4 && u32le(&b[..4])? as usize == b.len(),
        "Invalid table size"
    );
    let mut offsets = Vec::with_capacity(count + 1);
    for i in 0..count {
        offsets.push(u32le(&b[4 + i * 4..8 + i * 4])? as usize);
    }
    ensure!(offsets[0] == 4 + count * 4, "Wrong table field count");
    offsets.push(b.len());
    ensure!(
        offsets.windows(2).all(|p| p[0] <= p[1] && p[1] <= b.len()),
        "Invalid table offsets"
    );
    Ok(offsets.windows(2).map(|p| &b[p[0]..p[1]]).collect())
}
pub fn field(id: u32, kind: u8) -> [u8; 32] {
    let mut k = [0; 32];
    k[..4].copy_from_slice(&id.to_le_bytes());
    k[4] = kind;
    k
}
pub fn kv(id: u32, key: &[u8]) -> [u8; 32] {
    let mut h = gw_hash::blake2b::new_blake2b();
    h.update(&id.to_le_bytes());
    h.update(&[0]);
    h.update(key);
    let mut out = [0; 32];
    h.finalize(&mut out);
    out
}
pub fn balance_key(id: u32, short: &[u8]) -> [u8; 32] {
    let mut k = Vec::with_capacity(8 + short.len());
    k.extend_from_slice(&1u32.to_le_bytes());
    k.extend_from_slice(&(short.len() as u32).to_le_bytes());
    k.extend_from_slice(short);
    kv(id, &k)
}
pub struct V0Db {
    pub tree: SMT<ReadStore>,
    rebuilt: Option<SMT<gw_smt::smt::default_store::DefaultStore<H256>>>,
    pub tip: Value,
    pub metadata: Value,
}
impl V0Db {
    pub fn open(path: &Path) -> Result<Self> {
        ensure!(
            path.join("CURRENT").is_file(),
            "Expected existing RocksDB CURRENT"
        );
        let mut opts = Options::default();
        opts.create_if_missing(false);
        opts.set_max_open_files(2048);
        let cache = rocksdb::Cache::new_lru_cache(256 * 1024 * 1024);
        let mut blocks = rocksdb::BlockBasedOptions::default();
        blocks.set_block_cache(&cache);
        opts.set_block_based_table_factory(&blocks);
        let names = DB::list_cf(&opts, path)?;
        for c in [0, 2, 4, 9, 10, 14, 15, 25] {
            ensure!(names.contains(&c.to_string()), "Missing CF {c}");
        }
        let db = Arc::new(DB::open_cf_with_opts_for_read_only(
            &opts,
            path,
            names.iter().map(|name| {
                let mut cf = Options::default();
                cf.set_block_based_table_factory(&blocks);
                (name, cf)
            }),
            false,
        )?);
        let store = ReadStore { db };
        let chain = store
            .get(0, b"CHAIN_ID")?
            .context("Missing chain identity")?;
        ensure!(hx(chain) == ROLLUP, "Not mainnet v0 DB");
        let tip = store
            .get(0, b"LAST_VALID_TIP_BLOCK_HASH")?
            .context("No valid tip")?;
        let block = store.get(2, &tip)?.context("No tip block")?;
        let outer = table(&block, 6)?;
        let raw = table(outer[0], 10)?;
        ensure!(
            hash(outer[0]).as_slice() == tip,
            "Tip preimage/hash mismatch"
        );
        ensure!(raw[6].len() == 36, "Bad post-account state");
        let tip = json!({"hash":hx(&tip),"number":u64le(raw[0])?,"timestamp_ms":u64le(raw[4])?,"account_root":hx(&raw[6][..32]),"account_count":u32le(&raw[6][32..])?});
        let metadata = json!({"column_families":names,"db_version":store.get(0,b"db-version")?.map(hx),"tip_hash":store.get(0,b"TIP_BLOCK_HASH")?.map(hx)});
        let tree = SMT::new_with_store(store)?;
        Ok(Self {
            tree,
            rebuilt: None,
            tip,
            metadata,
        })
    }
    /// Rebuild every current leaf in an in-memory trie and require the committed root.
    /// The original v0 branch CF contains long uncompressed paths; do not repeatedly
    /// seek those 7+ GiB of branches when auditing millions of absent balance keys.
    pub fn rebuild(&mut self) -> Result<usize> {
        let db = &self.tree.store().db;
        let cf = db.cf_handle("10").context("leaf CF")?;
        let mut options = rocksdb::ReadOptions::default();
        options.fill_cache(false);
        let mut leaves = Vec::new();
        for item in db.iterator_cf_opt(cf, options, rocksdb::IteratorMode::Start) {
            let (k, v) = item?;
            let key: [u8; 32] = k.as_ref().try_into().context("leaf key width")?;
            let val: [u8; 32] = v.as_ref().try_into().context("leaf value width")?;
            leaves.push((H256::from(key), H256::from(val)));
        }
        let count = leaves.len();
        eprintln!("v0 rebuild: {count} stored leaves");
        let mut memory = SMT::<gw_smt::smt::default_store::DefaultStore<H256>>::default();
        memory.update_all(leaves)?;
        ensure!(
            memory.root() == self.tree.root(),
            "Full leaf rebuild root mismatch"
        );
        self.rebuilt = Some(memory);
        eprintln!("v0 full leaf rebuild verified");
        Ok(count)
    }
    pub fn state_store(&self) -> &dyn StoreReadOps<H256> {
        match &self.rebuilt {
            Some(t) => t.store(),
            None => self.tree.store(),
        }
    }
    pub fn statistics(&self) -> Result<Value> {
        let db = &self.tree.store().db;
        let mut rows = Vec::new();
        for name in ["6", "9", "10", "14", "15"] {
            let cf = db.cf_handle(name).context("CF")?;
            rows.push(json!({"cf":name,
                "estimated_keys":db.property_int_value_cf(cf, "rocksdb.estimate-num-keys")?,
                "live_sst_bytes":db.property_int_value_cf(cf, "rocksdb.total-sst-files-size")?}));
        }
        Ok(json!({"root":self.root(),"columns":rows}))
    }
    pub fn root(&self) -> String {
        hx(self.tree.root().as_slice())
    }
    pub fn raw_many(&self, keys: &[[u8; 32]]) -> Result<Vec<[u8; 32]>> {
        if keys.is_empty() {
            return Ok(vec![]);
        }
        let keys: Vec<H256> = keys.iter().copied().map(Into::into).collect();
        if let Some(memory) = &self.rebuilt {
            let leaves = keys
                .iter()
                .map(|k| Ok((*k, memory.get(k)?)))
                .collect::<Result<Vec<_>>>()?;
            let proof = memory.merkle_proof(keys)?;
            ensure!(
                proof.verify::<Blake2bHasher>(memory.root(), leaves.clone())?,
                "Rebuilt proof failed"
            );
            return Ok(leaves.into_iter().map(|(_, v)| v.into()).collect());
        }
        let leaves: Vec<_> = keys
            .iter()
            .map(|k| Ok((*k, self.tree.store().get_leaf(k)?.unwrap_or_default())))
            .collect::<Result<_>>()?;
        let proof = self.tree.merkle_proof(keys)?;
        ensure!(
            proof.verify::<Blake2bHasher>(self.tree.root(), leaves.clone())?,
            "v0 SMT inclusion/noninclusion proof failed"
        );
        Ok(leaves.into_iter().map(|(_, v)| v.into()).collect())
    }
    pub fn raw(&self, key: [u8; 32]) -> Result<[u8; 32]> {
        Ok(self.raw_many(&[key])?[0])
    }
    pub fn preimage(&self, col: usize, h: &[u8; 32]) -> Result<Option<Vec<u8>>> {
        let b = self.tree.store().get(col, h)?;
        if let Some(ref b) = b {
            ensure!(hash(b) == *h, "Preimage mismatch CF {col}");
        }
        Ok(b)
    }
}
pub fn capture(endpoint: &str) -> Result<Value> {
    capture_at(
        endpoint,
        endpoint,
        OutPoint {
            tx_hash: "0xc8502347882e91a849146f20dcff59d413056be9093c83ea17ed88339596f142".into(),
            index: "0x0".into(),
        },
    )
}
pub fn capture_at(endpoint: &str, indexer: &str, cp: OutPoint) -> Result<Value> {
    let rpc = Rpc::new(endpoint)?;
    let idx = Rpc::new(indexer)?;
    ensure!(
        rpc.call("get_header_by_number", json!(["0x0"]))?["hash"] == crate::claim::GENESIS,
        "Not CKB mainnet"
    );
    let itip = idx.call("get_indexer_tip", json!([]))?;
    ensure!(
        itip["block_hash"].is_string()
            && rpc.call("get_header_by_number", json!([itip["block_number"]]))?["hash"]
                == itip["block_hash"],
        "Indexer fork mismatch"
    );
    let before = rpc.call("get_tip_header", json!([]))?;
    let ty = Script {
        code_hash: "0xa9267ff5a16f38aa9382608eb9022883a78e6a40855107bb59f8406cce00e981".into(),
        hash_type: "type".into(),
        args: "0x2d8d67c8d73453c1a6d6d600e491b303910802e0cc90a709da9b15d26c5c48b3".into(),
    };
    ensure!(ty.hash()? == ROLLUP, "v0 type identity");
    let cells = idx.cells(&ty, "type", "exact")?;
    ensure!(cells.len() == 1, "Expected one rollup");
    let roll = &cells[0];
    let config = rpc.call("get_live_cell", json!([cp, true]))?;
    ensure!(config["status"] == "live", "Config cell not live");
    let data = config["cell"]["data"]["content"]
        .as_str()
        .context("Config data")?;
    let b = bytes(data)?;
    let fields = table(&b, 14)?;
    let state = bytes(&roll.output_data)?;
    ensure!(
        state.len() == 190,
        "Expected observed v0 GlobalState format"
    );
    ensure!(hash(&b).as_slice() == &state[..32], "Config hash mismatch");
    let mut families = BTreeMap::new();
    for (family, i) in [
        ("custodian", 1),
        ("deposit", 2),
        ("withdrawal", 3),
        ("challenge", 4),
        ("stake", 5),
    ] {
        ensure!(fields[i].len() == 32, "Bad code hash");
        let s = Script {
            code_hash: hx(fields[i]),
            hash_type: "type".into(),
            args: ROLLUP.into(),
        };
        families.insert(family, idx.cells(&s, "lock", "prefix")?);
    }
    let mut checked = 0;
    for c in std::iter::once(roll).chain(families["custodian"].iter()) {
        let r = rpc.call("get_live_cell", json!([c.out_point, true]))?;
        ensure!(
            r["status"] == "live"
                && r["cell"]["output"] == serde_json::to_value(&c.output)?
                && r["cell"]["data"]["content"] == c.output_data,
            "Cell changed during capture"
        );
        checked += 1;
    }
    ensure!(
        idx.cells(&ty, "type", "exact")? == cells,
        "Rollup changed during capture"
    );
    Ok(
        json!({"schema":"v0-readonly-capture-1","network":"mainnet_v0","observed_at":now(),"rollup_cell":roll,"config_data":data,"cells":families,"live_cells_checked":checked,"header_before":before,"header_after":rpc.call("get_tip_header",json!([]))?,"consistency":"rollup and all custodians checked live; other families are paginated indexer observations, not individually live-checked; non-atomic inventory"}),
    )
}
pub fn inspect(path: &Path, snapshot: Option<&Path>) -> Result<Value> {
    let db = V0Db::open(path)?;
    let mut r = json!({"network":"mainnet_v0","observed_at":now(),"db":db.metadata,"tip":db.tip,"stored_root":db.root(),"stored_root_matches_db_tip":db.root()==db.tip["account_root"],"read_only":true});
    if let Some(p) = snapshot {
        let v: Value = serde_json::from_slice(&fs::read(p)?)?;
        l1_report(&v)?;
        let state = bytes(v["rollup_cell"]["output_data"].as_str().context("state")?)?;
        ensure!(state.len() == 190, "V0 state size");
        r["l1_root"] = json!(hx(&state[32..64]));
        r["l1_tip"] = json!(hx(&state[140..172]));
        r["l1_account_count"] = json!(u32le(&state[64..68])?);
        r["l1_block_count"] = json!(u64le(&state[100..108])?);
        r["matches_l1"] = json!(
            r["l1_root"] == r["stored_root"]
                && r["l1_tip"] == db.tip["hash"]
                && r["l1_account_count"] == db.tip["account_count"]
                && r["l1_block_count"].as_u64() == db.tip["number"].as_u64().map(|n| n + 1)
        );
    }
    Ok(r)
}
fn write_json(path: &Path, v: &Value) -> Result<()> {
    use std::io::Write;
    let mut f = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)?;
    f.write_all(&serde_json::to_vec_pretty(v)?)?;
    Ok(())
}
fn mapping_key(kind: u8, tail: &[u8]) -> [u8; 32] {
    let mut v = Vec::with_capacity(5 + tail.len());
    v.extend_from_slice(&[0, 0, 0, 0, kind]);
    v.extend_from_slice(tail);
    hash(&v)
}
pub fn inventory(path: &Path, snapshot: &Path, output: &Path) -> Result<Value> {
    use gw_types::{packed, prelude::*};
    use num_bigint::BigUint;
    ensure!(!output.exists(), "Output exists");
    let d = path.canonicalize()?;
    let parent = output.parent().context("Output parent")?.canonicalize()?;
    ensure!(!parent.starts_with(&d), "Output inside DB");
    let check = inspect(path, Some(snapshot))?;
    ensure!(
        check["stored_root_matches_db_tip"] == true
            && check["stored_root"] == check["l1_root"]
            && check["tip"]["account_count"] == check["l1_account_count"],
        "Account root/count mismatch"
    );
    let mut db = V0Db::open(path)?;
    let rebuilt_leaves = db.rebuild()?;
    let n = u32::try_from(db.tip["account_count"].as_u64().context("count")?)?;
    ensure!(n < 1_000_000, "Count limit");
    let snap: Value = serde_json::from_slice(&fs::read(snapshot)?)?;
    l1_report(&snap)?;
    let state = bytes(
        snap["rollup_cell"]["output_data"]
            .as_str()
            .context("state")?,
    )?;
    let finalized = u64le(&state[180..188])?;
    fs::create_dir(output)?;
    fs::create_dir(output.join("runtime"))?;
    write_json(&output.join("root-check.json"), &check)?;
    let mut accounts = Vec::with_capacity(n as usize);
    let mut shs = Vec::with_capacity(n as usize);
    let mut assets = Vec::new();
    let mut counts = BTreeMap::<String, u32>::new();
    let mut verified = 0u64;
    let mut groups = BTreeMap::<String, Vec<u32>>::new();
    let mut shorts = std::collections::BTreeSet::new();
    for start in (0..n).step_by(256) {
        let end = (start + 256).min(n);
        let keys: Vec<_> = (start..end).map(|id| field(id, 2)).collect();
        let hashes = db.raw_many(&keys)?;
        verified += keys.len() as u64;
        for (id, sh) in (start..end).zip(hashes) {
            ensure!(sh != [0; 32], "Missing account {id}");
            let b = db.preimage(14, &sh)?.context("Missing Script preimage")?;
            let script = Script::from_packed(packed::Script::from_slice(&b)?)?;
            let args = bytes(&script.args)?;
            let prefix = args.starts_with(&hash32(ROLLUP)?);
            let kind = if script.hash_type != "type" || !prefix {
                "unknown"
            } else if script.code_hash == ETH && args.len() == 52 {
                "eth_eoa"
            } else if script.code_hash == TRON && args.len() == 52 {
                "tron_deferred"
            } else if script.code_hash == SUDT && args.len() == 64 {
                "native_sudt"
            } else if script.code_hash == POLY {
                "polyjuice"
            } else {
                "other_system_or_unknown"
            };
            ensure!(
                shorts.insert(sh[..20].to_vec()),
                "Short-address collision; manual review required"
            );
            let owner = if kind == "eth_eoa" {
                Some(hx(&args[32..]))
            } else {
                None
            };
            let mut row = json!({"account_id":id,"script_hash":hx(sh),"l2_short_address":hx(&sh[..20]),"kind":kind,"owner_eth_address":owner,"script":script});
            if kind == "native_sudt" {
                assets.push(
                    json!({"account_id":id,"l1_type_hash":hx(&args[32..]),"script_hash":hx(sh)}),
                );
            }
            if kind == "polyjuice" {
                let mut k = field(id, 255);
                k[5] = 1;
                let code = db.raw(kv(id, &k))?;
                k[5] = 2;
                let destructed = db.raw(kv(id, &k))?;
                verified += 2;
                row["runtime_hash"] = json!(hx(code));
                row["destructed_flag"] = json!(hx(destructed));
                if code != [0; 32] {
                    groups.entry(hx(code)).or_default().push(id);
                    let file = output
                        .join("runtime")
                        .join(format!("{}.bin", hex::encode(code)));
                    if let Some(b) = db.preimage(15, &code)? {
                        row["runtime_bytes"] = json!(b.len());
                        if !file.exists() {
                            fs::write(file, &b)?;
                        }
                    } else if code == hash(&[]) {
                        row["runtime_bytes"] = json!(0);
                    } else {
                        row["runtime_missing"] = json!(true);
                    }
                }
            }
            *counts.entry(kind.into()).or_default() += 1;
            accounts.push(row);
            shs.push(sh);
        }
        if start % 4096 == 0 {
            eprintln!("v0 account identities {end}/{n}");
        }
    }
    // Authenticate both legacy mapping domains; id 0 legitimately encodes the zero value.
    for start in (0..n as usize).step_by(256) {
        let end = (start + 256).min(n as usize);
        let mut keys = Vec::new();
        for sh in &shs[start..end] {
            keys.push(mapping_key(3, sh));
            let mut suffix = 20u32.to_le_bytes().to_vec();
            suffix.extend_from_slice(&sh[..20]);
            keys.push(mapping_key(5, &suffix));
        }
        let values = db.raw_many(&keys)?;
        verified += keys.len() as u64;
        if start % 4096 == 0 {
            eprintln!("v0 account mappings {end}/{n}");
        }
        for (off, pair) in values.chunks_exact(2).enumerate() {
            let id = (start + off) as u32;
            let mut expected = [0; 32];
            expected[..4].copy_from_slice(&id.to_le_bytes());
            ensure!(
                pair[0] == expected && pair[1] == shs[id as usize],
                "v0 account mapping mismatch for {id}"
            );
        }
    }
    write_json(&output.join("accounts.json"), &json!(accounts))?;
    write_json(&output.join("runtime-groups.json"), &json!(groups))?;
    // L1 inventory keeps unfinalized custody and separate cell families separate.
    let custodians: Vec<Cell> = serde_json::from_value(snap["cells"]["custodian"].clone())?;
    let mut custody = BTreeMap::<String, (BigUint, BigUint)>::new();
    let mut types = BTreeMap::<String, Script>::new();
    let mut custody_rows = Vec::new();
    for c in &custodians {
        let args = bytes(&c.output.lock.args)?;
        ensure!(args.starts_with(&hash32(ROLLUP)?), "Foreign custodian");
        let a = table(&args[32..], 3)?;
        let dep = u64le(a[2])?;
        let eligible = dep <= finalized;
        let capacity = BigUint::from(quantity(&c.output.capacity)?);
        let e = custody.entry(NATIVE.into()).or_default();
        e.0 += &capacity;
        if eligible {
            e.1 += &capacity;
        }
        let mut token = None;
        if let Some(t) = &c.output.type_script {
            let th = t.hash()?;
            let b = bytes(&c.output_data)?;
            ensure!(b.len() == 16, "Noncanonical custodian token data");
            let amount = u128::from_le_bytes(b.as_slice().try_into()?);
            let e = custody.entry(th.clone()).or_default();
            e.0 += BigUint::from(amount);
            if eligible {
                e.1 += BigUint::from(amount);
            }
            types.insert(th.clone(), t.clone());
            token = Some(json!({"type_hash":th,"amount":amount.to_string()}));
        } else {
            ensure!(
                bytes(&c.output_data)?.is_empty(),
                "Nonempty plain custodian"
            );
        }
        custody_rows.push(json!({"out_point":c.out_point,"deposit_block_number":dep,"eligible_finalized":eligible,"capacity_shannons":capacity.to_string(),"token":token}));
    }
    write_json(&output.join("custodian-cells.json"), &json!(custody_rows))?;
    let mut balance_csv = csv::Writer::from_path(output.join("balances.csv"))?;
    balance_csv.write_record([
        "account_id",
        "kind",
        "l2_short_address",
        "owner_eth_address",
        "asset_account_id",
        "l1_type_hash",
        "raw_amount",
        "has_finalized_custodian",
    ])?;
    let mut summaries = Vec::new();
    let mut funded = std::collections::BTreeSet::new();
    let mut funded_contracts = std::collections::BTreeSet::new();
    let mut user_ckb = vec![0u128; n as usize];
    let mut min_ckb = vec![0u128; n as usize];
    for a in &assets {
        let aid = a["account_id"].as_u64().unwrap() as u32;
        let th = a["l1_type_hash"].as_str().unwrap();
        let mut sum = BigUint::default();
        let mut categories = BTreeMap::<String, BigUint>::new();
        let mut positives = 0;
        let (total, available) = custody.get(th).cloned().unwrap_or_default();
        let backed = available > BigUint::default();
        for start in (0..n as usize).step_by(512) {
            let end = (start + 512).min(n as usize);
            let keys: Vec<_> = shs[start..end]
                .iter()
                .map(|sh| balance_key(aid, &sh[..20]))
                .collect();
            let vals = db.raw_many(&keys)?;
            verified += keys.len() as u64;
            for (off, v) in vals.iter().enumerate() {
                ensure!(v[16..] == [0; 16], "Noncanonical u128 balance");
                let amount = u128::from_le_bytes(v[..16].try_into()?);
                if amount == 0 {
                    continue;
                }
                let id = start + off;
                let row = &accounts[id];
                let kind = row["kind"].as_str().unwrap();
                sum += BigUint::from(amount);
                *categories.entry(kind.into()).or_default() += BigUint::from(amount);
                positives += 1;
                funded.insert(id);
                if kind == "polyjuice" {
                    funded_contracts.insert(id);
                }
                if aid == 1 {
                    ensure!(th == NATIVE, "Native CKB identity mismatch");
                    user_ckb[id] = amount;
                } else if backed {
                    let ty = types.get(th).context("Missing asset Script")?;
                    min_ckb[id] = min_ckb[id]
                        .checked_add(
                            (8 + 55 + 33 + bytes(&ty.args)?.len() + 16) as u128 * 100_000_000,
                        )
                        .context("capacity overflow")?;
                }
                balance_csv.write_record([
                    id.to_string(),
                    kind.to_owned(),
                    row["l2_short_address"].as_str().unwrap().to_owned(),
                    row["owner_eth_address"].as_str().unwrap_or("").to_owned(),
                    aid.to_string(),
                    th.to_owned(),
                    amount.to_string(),
                    backed.to_string(),
                ])?;
            }
        }
        let sufficient = available >= sum;
        let delta = if sufficient {
            (&available - &sum).to_string()
        } else {
            format!("-{}", &sum - &available)
        };
        summaries.push(json!({"account_id":aid,"l1_type_hash":th,"registered_account_sum":sum.to_string(),"positive_accounts":positives,"by_account_kind":categories.into_iter().map(|(k,v)|(k,v.to_string())).collect::<BTreeMap<_,_>>(),"custodian_total":total.to_string(),"finalized_custodian_total":available.to_string(),"finalized_minus_registered":delta,"covers_registered_balances":sufficient,"full_liability_coverage":"not_established_unmapped_keys_and_protocol_analysis_pending"}));
        eprintln!("v0 asset {aid}: {positives} funded accounts; finalized covers registered = {sufficient}");
    }
    balance_csv.flush()?;
    let mut capacity = Vec::new();
    for (i, a) in accounts.iter().enumerate() {
        if a["kind"] != "eth_eoa" {
            continue;
        }
        let minimum = if min_ckb[i] > 0 {
            min_ckb[i]
        } else if user_ckb[i] > 0 {
            63 * 100_000_000
        } else {
            0
        };
        if minimum > 0 {
            capacity.push(json!({"account_id":i,"owner_eth_address":a["owner_eth_address"],"own_ckb_shannons":user_ckb[i].to_string(),"minimum_output_capacity_shannons":minimum.to_string(),"shortfall_shannons":minimum.saturating_sub(user_ckb[i]).to_string(),"scope":"direct supported balances only; excludes transaction fee and indirect positions"}));
        }
    }
    let unmapped:Vec<_>=custody.iter().filter(|(th,_)|!assets.iter().any(|a|a["l1_type_hash"]==**th)).map(|(th,(a,b))|json!({"l1_type_hash":th,"total":a.to_string(),"finalized":b.to_string()})).collect();
    write_json(&output.join("asset-reconciliation.json"), &json!(summaries))?;
    write_json(&output.join("capacity.json"), &json!(capacity))?;
    let result = json!({"network":"mainnet_v0","root":db.root(),"tip":db.tip,"root_checks":check,"account_count":n,"account_kinds":counts,"native_asset_count":assets.len(),"funded_accounts":funded.len(),"funded_polyjuice_accounts":funded_contracts.len(),"funded_polyjuice_ids":funded_contracts,"runtime_groups":groups.len(),"full_leaf_rebuild_verified":true,"stored_leaves_rebuilt":rebuilt_leaves,"authenticated_leaf_reads":verified,"custodian_cell_count":custodians.len(),"custodian_assets_without_native_match":unmapped,"other_l1_cell_counts":snap["cells"].as_object().unwrap().iter().map(|(k,v)|(k.clone(),v.as_array().unwrap().len())).collect::<BTreeMap<_,_>>(),"limitations":["Registered-address sums are not exhaustive supply; unmapped/nonstandard addresses still require transaction-history discovery.","No protocol-derived allocation or payout approval.","A matching account root does not erase an independently reported block-tip mismatch."]});
    write_json(&output.join("summary.json"), &result)?;
    Ok(result)
}
fn encode_table(fields: &[Vec<u8>]) -> Result<Vec<u8>> {
    let header = 4 + fields.len() * 4;
    let total = header + fields.iter().map(Vec::len).sum::<usize>();
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(&u32::try_from(total)?.to_le_bytes());
    let mut offset = header;
    for f in fields {
        out.extend_from_slice(&u32::try_from(offset)?.to_le_bytes());
        offset += f.len();
    }
    for f in fields {
        out.extend_from_slice(f);
    }
    Ok(out)
}
/// Authenticate the observed one-block DB lag through the L1-committed raw block hash.
pub fn verify_empty_successor(db_tip: &Value, response: &Value, state: &[u8]) -> Result<Value> {
    ensure!(state.len() == 190, "GlobalState size");
    let b = &response["result"]["block"];
    let r = &b["raw"];
    let q = |v: &Value| -> Result<u64> { quantity(v.as_str().context("hex quantity")?) };
    let h = |v: &Value| -> Result<Vec<u8>> { Ok(hash32(v.as_str().context("hash")?)?.to_vec()) };
    let account = |v: &Value| -> Result<Vec<u8>> {
        let mut out = h(&v["merkle_root"])?;
        out.extend_from_slice(&u32::try_from(q(&v["count"])?)?.to_le_bytes());
        Ok(out)
    };
    let prev = account(&r["prev_account"])?;
    let post = account(&r["post_account"])?;
    ensure!(
        prev == post && post.as_slice() == &state[32..68],
        "Successor changes account state"
    );
    ensure!(
        r["parent_block_hash"] == db_tip["hash"]
            && q(&r["number"])? == db_tip["number"].as_u64().context("DB number")? + 1,
        "Not direct successor"
    );
    ensure!(
        r["prev_account"]["merkle_root"] == db_tip["account_root"]
            && q(&r["prev_account"]["count"])?
                == db_tip["account_count"].as_u64().context("DB count")?,
        "Wrong previous state"
    );
    ensure!(
        q(&r["number"])? + 1 == u64le(&state[100..108])?,
        "L1 block count mismatch"
    );
    let empty = |v: &Value| v.as_array().map(|v| v.is_empty()).unwrap_or(false);
    ensure!(
        empty(&b["transactions"])
            && empty(&b["withdrawal_requests"])
            && empty(&r["state_checkpoint_list"]),
        "Not empty successor"
    );
    let tx = &r["submit_transactions"];
    let wd = &r["submit_withdrawals"];
    ensure!(
        q(&tx["tx_count"])? == 0 && q(&wd["withdrawal_count"])? == 0,
        "Nonempty counts"
    );
    let mut withdrawals = h(&wd["withdrawal_witness_root"])?;
    withdrawals.extend_from_slice(&0u32.to_le_bytes());
    let transactions = encode_table(&[
        h(&tx["tx_witness_root"])?,
        0u32.to_le_bytes().to_vec(),
        h(&tx["prev_state_checkpoint"])?,
    ])?;
    let raw = encode_table(&[
        q(&r["number"])?.to_le_bytes().to_vec(),
        u32::try_from(q(&r["block_producer_id"])?)?
            .to_le_bytes()
            .to_vec(),
        h(&r["parent_block_hash"])?,
        h(&r["stake_cell_owner_lock_hash"])?,
        q(&r["timestamp"])?.to_le_bytes().to_vec(),
        prev,
        post,
        0u32.to_le_bytes().to_vec(),
        withdrawals,
        transactions,
    ])?;
    ensure!(
        hash(&raw).as_slice() == &state[140..172],
        "Successor raw block hash is not L1 committed tip"
    );
    Ok(
        json!({"verified":true,"db_block_number":db_tip["number"],"l1_block_number":q(&r["number"])?,"l1_tip_hash":hx(hash(&raw)),"same_account_root_and_count":true,"transactions":0,"withdrawals":0,"meaning":"DB account state is current at L1 commitment; DB block history remains one block behind"}),
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn v0_key_and_ckb_encoding_golden() {
        assert_eq!(
            hx(balance_key(
                1,
                &hex::decode("deb3312e58616e345acf9bef412a8c13d9ab4fb4").unwrap()
            )),
            "0x05ba1b5da789673642ee88f193a9947c324a2d747571860160e5b909d71e58e7"
        );
        let value = 123456789u128.to_le_bytes();
        assert_eq!(&value[..4], &[0x15, 0xcd, 0x5b, 0x07]);
    }
    #[test]
    fn v0_committed_empty_successor_and_negative_cases() {
        let f: Value =
            serde_json::from_slice(include_bytes!("../tests/fixtures/v0/empty-successor.json"))
                .unwrap();
        let state = bytes(f["l1_state"].as_str().unwrap()).unwrap();
        assert!(verify_empty_successor(&f["db_tip"], &f["block_response"], &state).is_ok());
        let mut wrong = f["block_response"].clone();
        wrong["result"]["block"]["raw"]["parent_block_hash"] = json!(hx([0; 32]));
        assert!(verify_empty_successor(&f["db_tip"], &wrong, &state).is_err());
        let mut wrong = f["block_response"].clone();
        wrong["result"]["block"]["raw"]["post_account"]["count"] = json!("0xc341");
        assert!(verify_empty_successor(&f["db_tip"], &wrong, &state).is_err());
        let mut wrong = f["block_response"].clone();
        wrong["result"]["block"]["withdrawal_requests"] = json!([{}]);
        assert!(verify_empty_successor(&f["db_tip"], &wrong, &state).is_err());
        let mut bad_state = state.clone();
        bad_state[140] ^= 1;
        assert!(verify_empty_successor(&f["db_tip"], &f["block_response"], &bad_state).is_err());
    }
    #[test]
    fn v0_l1_capture_rejects_network_duplicates_and_foreign_assets() {
        let f: Value =
            serde_json::from_slice(include_bytes!("../tests/fixtures/v0/l1-validation.json"))
                .unwrap();
        assert!(l1_report(&f).is_ok());
        let mut wrong = f.clone();
        wrong["network"] = json!("mainnet_v1");
        assert!(l1_report(&wrong).is_err());
        let mut wrong = f.clone();
        let first = wrong["cells"]["custodian"][0].clone();
        wrong["cells"]["custodian"]
            .as_array_mut()
            .unwrap()
            .push(first);
        assert!(l1_report(&wrong).is_err());
        let mut wrong = f.clone();
        wrong["cells"]["custodian"][0]["output"]["lock"]["code_hash"] = json!(hx([0; 32]));
        assert!(l1_report(&wrong).is_err());
        let mut wrong = f.clone();
        wrong["cells"]["custodian"][0]["output"]["type"]["args"] = json!("0x00");
        assert!(l1_report(&wrong).is_err());
        let mut wrong = f.clone();
        wrong["cells"]["custodian"][0]["output_data"] = json!("0x00");
        assert!(l1_report(&wrong).is_err());
        let mut wrong = f.clone();
        wrong["config_data"] = json!("0x00");
        assert!(l1_report(&wrong).is_err());
    }
    #[test]
    fn yokai_protocol_fee_matches_live_case_and_zero_growth() {
        use num_bigint::BigUint;
        let n = |s: &str| s.parse::<BigUint>().unwrap();
        let (s, a, b, k) = (
            n("1343948356662017878"),
            n("151695502949394402424"),
            n("18666128110852231"),
            n("2679424506968933068158335660792813264"),
        );
        assert_eq!(yokai_fee(&s, &a, &b, &k, true), n("11816390524256928"));
        assert_eq!(yokai_fee(&s, &a, &b, &k, false), n("0"));
        assert_eq!(yokai_fee(&s, &a, &b, &n("0"), true), n("0"));
        assert_eq!(yokai_fee(&s, &a, &b, &(&a * &b), true), n("0"));
    }
    #[test]
    fn v0_tables_reject_schema_and_bounds() {
        let b = encode_table(&[vec![1, 2], vec![3]]).unwrap();
        assert_eq!(table(&b, 2).unwrap(), vec![&[1, 2][..], &[3][..]]);
        assert!(table(&b, 3).is_err());
        assert!(table(&b, 0).is_err());
        for n in 0..b.len() {
            assert!(table(&b[..n], 2).is_err());
        }
        let mut corrupt = b.clone();
        corrupt[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(table(&corrupt, 2).is_err());
    }
}
/// Authenticate saved capture identities/config and summarize liabilities outside custody.
pub fn l1_report(snap: &Value) -> Result<Value> {
    use num_bigint::BigUint;
    ensure!(snap["network"] == "mainnet_v0", "Wrong network");
    let roll: Cell = serde_json::from_value(snap["rollup_cell"].clone())?;
    ensure!(
        roll.output
            .type_script
            .as_ref()
            .context("rollup type")?
            .hash()?
            == ROLLUP,
        "Wrong rollup Script"
    );
    let state = bytes(&roll.output_data)?;
    ensure!(
        state.len() == 190 && state[189] == 1,
        "Unsupported v0 GlobalState"
    );
    let cb = bytes(snap["config_data"].as_str().context("config")?)?;
    ensure!(hash(&cb).as_slice() == &state[..32], "Config hash mismatch");
    let config = table(&cb, 14)?;
    ensure!(hx(config[6]) == SUDT, "Unknown v0 native validator");
    let finalized = u64le(&state[180..188])?;
    let mut totals = Vec::new();
    let mut withdrawals = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for (family, index) in [
        ("custodian", 1),
        ("deposit", 2),
        ("withdrawal", 3),
        ("challenge", 4),
        ("stake", 5),
    ] {
        let cells: Vec<Cell> = serde_json::from_value(snap["cells"][family].clone())?;
        let mut sums = BTreeMap::<String, BigUint>::new();
        let mut unfinalized = 0;
        for c in &cells {
            ensure!(seen.insert(c.out_point.clone()), "Duplicate L1 outpoint");
            let args = bytes(&c.output.lock.args)?;
            ensure!(
                c.output.lock.code_hash == hx(config[index])
                    && c.output.lock.hash_type == "type"
                    && args.starts_with(&hash32(ROLLUP)?),
                "Foreign family Script"
            );
            *sums.entry(NATIVE.into()).or_default() += BigUint::from(quantity(&c.output.capacity)?);
            let th = if let Some(t) = &c.output.type_script {
                ensure!(
                    t.hash_type == "type"
                        && t.code_hash == hx(config[0])
                        && bytes(&t.args)?.len() == 32,
                    "Unknown L1 asset type"
                );
                let th = t.hash()?;
                let d = bytes(&c.output_data)?;
                ensure!(d.len() == 16, "Noncanonical sUDT data");
                *sums.entry(th.clone()).or_default() +=
                    BigUint::from(u128::from_le_bytes(d.as_slice().try_into()?));
                th
            } else {
                ensure!(bytes(&c.output_data)?.is_empty(), "Plain cell with data");
                NATIVE.into()
            };
            if family == "withdrawal" {
                let a = &args[32..];
                ensure!(a.len() == 192, "Unknown v0 withdrawal format");
                let block = u64le(&a[64..72])?;
                if block > finalized {
                    unfinalized += 1;
                }
                ensure!(hx(&a[72..104]) == th, "Withdrawal asset mismatch");
                withdrawals.push(json!({"out_point":c.out_point,"l2_account_script_hash":hx(&a[..32]),"withdrawal_block_number":block,"finalized":block<=finalized,"owner_lock_hash":hx(&a[128..160]),"payment_lock_hash":hx(&a[160..192]),"sell_amount":u128::from_le_bytes(a[104..120].try_into()?).to_string(),"sell_capacity_shannons":u64le(&a[120..128])?.to_string(),"capacity_shannons":quantity(&c.output.capacity)?.to_string(),"l1_type_hash":th,"raw_token_amount":if c.output.type_script.is_some(){Some(u128::from_le_bytes(bytes(&c.output_data)?.as_slice().try_into()?).to_string())}else{None}}));
            }
        }
        totals.push(json!({"family":family,"cell_count":cells.len(),"asset_totals":sums.into_iter().map(|(k,v)|(k,v.to_string())).collect::<BTreeMap<_,_>>(),"unfinalized_withdrawal_cells":if family=="withdrawal"{Some(unfinalized)}else{None}}));
    }
    Ok(
        json!({"network":"mainnet_v0","finalized_block_number":finalized,"families":totals,"existing_withdrawals":withdrawals,"policy":"Existing withdrawals are separate L1 holdings, not another allocation from custodian. Preserve current owner/payment rights. Pending deposits require separate cancellation/reconciliation."}),
    )
}
/// Receipt logs are address-discovery hints; only current-root proofs establish balances.
pub fn discover_unmapped(path: &Path, inventory: &Path) -> Result<Value> {
    use num_bigint::BigUint;
    let db = V0Db::open(path)?;
    let summary: Value = serde_json::from_slice(&fs::read(inventory.join("summary.json"))?)?;
    ensure!(summary["root"] == db.root(), "Inventory root mismatch");
    let accounts: Vec<Value> = serde_json::from_slice(&fs::read(inventory.join("accounts.json"))?)?;
    let known: std::collections::BTreeSet<Vec<u8>> = accounts
        .iter()
        .map(|a| bytes(a["l2_short_address"].as_str().context("short")?))
        .collect::<Result<_>>()?;
    let assets: Vec<Value> =
        serde_json::from_slice(&fs::read(inventory.join("asset-reconciliation.json"))?)?;
    let ids: std::collections::BTreeSet<u32> = assets
        .iter()
        .map(|a| a["account_id"].as_u64().unwrap() as u32)
        .collect();
    let store = db.tree.store();
    let cf = store.db.cf_handle("6").context("receipt CF")?;
    let mut candidates = std::collections::BTreeSet::<(u32, Vec<u8>)>::new();
    let mut receipts = 0u64;
    let mut logs = 0u64;
    let mut opts = rocksdb::ReadOptions::default();
    opts.fill_cache(false);
    for item in store
        .db
        .iterator_cf_opt(cf, opts, rocksdb::IteratorMode::Start)
    {
        let (_, v) = item?;
        let fields = table(&v, 4)?;
        let raw = fields[3];
        ensure!(
            raw.len() >= 4 && u32le(&raw[..4])? as usize == raw.len(),
            "Log vector malformed"
        );
        if raw.len() > 4 {
            ensure!(raw.len() >= 8, "Log header");
            let first = u32le(&raw[4..8])? as usize;
            ensure!(
                first >= 8 && first % 4 == 0 && first <= raw.len(),
                "Log count"
            );
            for item in table(raw, first / 4 - 1)? {
                let l = table(item, 3)?;
                let id = u32le(l[0])?;
                ensure!(l[1].len() == 1, "Log flag size");
                if !ids.contains(&id) || l[1][0] > 1 {
                    continue;
                }
                let b = l[2];
                ensure!(
                    b.len() >= 5 && u32le(&b[..4])? as usize == b.len() - 4,
                    "Log bytes"
                );
                let b = &b[4..];
                let len = b[0] as usize;
                ensure!(
                    len <= 32 && b.len() == 1 + 2 * len + 16,
                    "Native log address layout"
                );
                for addr in [&b[1..1 + len], &b[1 + len..1 + 2 * len]] {
                    if !known.contains(addr) {
                        candidates.insert((id, addr.to_vec()));
                    }
                }
                logs += 1;
            }
        }
        receipts += 1;
        if receipts % 250000 == 0 {
            eprintln!(
                "v0 receipt discovery {receipts}; extra asset/address pairs {}",
                candidates.len()
            );
        }
    }
    let mut positives = Vec::new();
    let mut totals = BTreeMap::<u32, BigUint>::new();
    let candidates: Vec<_> = candidates.into_iter().collect();
    for chunk in candidates.chunks(512) {
        let keys: Vec<_> = chunk.iter().map(|(id, a)| balance_key(*id, a)).collect();
        let values = db.raw_many(&keys)?;
        for ((id, a), v) in chunk.iter().zip(values) {
            ensure!(v[16..] == [0; 16], "Noncanonical native balance");
            let amount = u128::from_le_bytes(v[..16].try_into()?);
            if amount > 0 {
                *totals.entry(*id).or_default() += BigUint::from(amount);
                positives.push(json!({"asset_account_id":id,"short_address":hx(a),"address_bytes":a.len(),"raw_amount":amount.to_string(),"disposition":"unmapped_owner_reserved_pending_analysis"}));
            }
        }
    }
    Ok(
        json!({"root":db.root(),"receipts_scanned":receipts,"native_logs_scanned":logs,"unmapped_asset_address_pairs_checked":candidates.len(),"positive_balances":positives,"extra_totals":totals.into_iter().map(|(k,v)|(k.to_string(),v.to_string())).collect::<BTreeMap<_,_>>(),"coverage":"Address preimages discovered from stored receipts and balances authenticated at current root; receipt-history completeness is not independently proven."}),
    )
}
pub fn probe(path: &Path, request: &Value) -> Result<Value> {
    let mut db = V0Db::open(path)?;
    ensure!(request["root"] == db.root(), "Probe root mismatch");
    db.rebuild()?;
    let mut rows = Vec::new();
    let list = request["accounts"].as_array().context("accounts")?;
    ensure!(list.len() <= 10000, "Probe count");
    for row in list {
        let id = u32::try_from(row["account_id"].as_u64().context("id")?)?;
        ensure!(
            (id as u64) < db.tip["account_count"].as_u64().context("count")?,
            "Probe account bound"
        );
        let sh = db.raw(field(id, 2))?;
        let slots = row["slots"].as_array().context("slots")?;
        ensure!(slots.len() <= 10000, "Slot bound");
        let application: Vec<_> = slots
            .iter()
            .map(|s| hash32(s.as_str().context("slot")?))
            .collect::<Result<_>>()?;
        let keys: Vec<_> = application.iter().map(|k| kv(id, k)).collect();
        let values = db.raw_many(&keys)?;
        rows.push(json!({"account_id":id,"script_hash":hx(sh),"slots":application.into_iter().zip(values).map(|(k,v)|json!({"key":hx(k),"value":hx(v)})).collect::<Vec<_>>()}));
    }
    Ok(json!({"root":db.root(),"accounts":rows,"all_reads_authenticated":true}))
}
pub(crate) fn yokai_fee(
    supply: &num_bigint::BigUint,
    reserve0: &num_bigint::BigUint,
    reserve1: &num_bigint::BigUint,
    k_last: &num_bigint::BigUint,
    enabled: bool,
) -> num_bigint::BigUint {
    use num_bigint::BigUint;
    if !enabled || *k_last == BigUint::default() {
        return BigUint::default();
    }
    let root = (reserve0 * reserve1).sqrt();
    let last = k_last.sqrt();
    if root <= last {
        return BigUint::default();
    }
    supply * (&root - &last) * 8u32 / (&root * 17u32 + &last * 8u32)
}
/// Enumerate known-account LP owners for the exact reviewed YokaiPair runtime.
/// Supply gaps and contract-held shares remain reserved; this is not a payout plan.
pub fn yokai_pairs(path: &Path, inventory: &Path) -> Result<Value> {
    let mut db = V0Db::open(path)?;
    db.rebuild()?;
    let accounts: Vec<Value> = serde_json::from_slice(&fs::read(inventory.join("accounts.json"))?)?;
    let summary: Value = serde_json::from_slice(&fs::read(inventory.join("summary.json"))?)?;
    ensure!(summary["root"] == db.root(), "Inventory root mismatch");
    yokai_pairs_from_db(&db, &accounts)
}

/// Shared offline reader: callers supply identities freshly enumerated from this DB.
pub(crate) fn yokai_pairs_from_db(db: &V0Db, accounts: &[Value]) -> Result<Value> {
    use num_bigint::BigUint;
    use sha3::{Digest, Keccak256};

    ensure!(
        accounts.len() as u64 == db.tip["account_count"].as_u64().context("count")?,
        "Account count"
    );
    for chunk in accounts.chunks(512) {
        let keys = chunk
            .iter()
            .map(|a| {
                Ok(field(
                    u32::try_from(a["account_id"].as_u64().context("id")?)?,
                    2,
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        for (a, sh) in chunk.iter().zip(db.raw_many(&keys)?) {
            ensure!(
                a["script_hash"] == hx(sh) && a["l2_short_address"] == hx(&sh[..20]),
                "Account preimage identity"
            );
        }
    }
    ensure!(
        accounts
            .iter()
            .enumerate()
            .all(|(i, a)| a["account_id"].as_u64() == Some(i as u64)),
        "Noncanonical account order"
    );
    let by_short: BTreeMap<_, _> = accounts
        .iter()
        .map(|a| (a["l2_short_address"].as_str().unwrap(), a))
        .collect();
    let slot = |n: u64| {
        let mut b = [0u8; 32];
        b[24..].copy_from_slice(&n.to_be_bytes());
        b
    };
    let pair_hash = "0xf601f6484fc84f0515c0ed834594c582a3974f00e4dc030ec489e8b57c75f8da";
    let factory_hash = "0x4ec3481f45a946241977d5338439efea01e7feff349aa56fbc78abcf8b0e3d36";
    let mut owners = accounts
        .iter()
        .map(|a| {
            Ok((
                Some(a["account_id"].as_u64().context("id")?),
                eth_address(a["l2_short_address"].as_str().context("short")?)?,
            ))
        })
        .collect::<Result<Vec<_>>>()?;
    owners.push((None, [0; 20]));
    let mut pairs = Vec::new();
    for a in accounts.iter().filter(|a| a["runtime_hash"] == pair_hash) {
        let id = a["account_id"].as_u64().unwrap() as u32;
        let mut code_key = field(id, 255);
        code_key[5] = 1;
        ensure!(
            db.raw(kv(id, &code_key))? == hash32(pair_hash)?,
            "Pair runtime"
        );
        let keys = (0..13).map(|n| kv(id, &slot(n))).collect::<Vec<_>>();
        let v = db.raw_many(&keys)?;
        let supply = BigUint::from_bytes_be(&v[0]);
        if supply == BigUint::default() {
            continue;
        }
        let factory = hx(&v[5][12..]);
        let token0 = hx(&v[6][12..]);
        let token1 = hx(&v[7][12..]);
        let reserve0 = BigUint::from_bytes_be(&v[8][18..]);
        let reserve1 = BigUint::from_bytes_be(&v[8][4..18]);
        let k_last = BigUint::from_bytes_be(&v[11]);
        let mut fee = BigUint::default();
        let mut fee_to = None;
        let mut fee_verified = false;
        if let Some(f) = by_short.get(factory.as_str()) {
            let fid = f["account_id"].as_u64().unwrap() as u32;
            let mut ck = field(fid, 255);
            ck[5] = 1;
            if db.raw(kv(fid, &ck))? == hash32(factory_hash)? {
                let to = db.raw(kv(fid, &slot(0)))?;
                fee_to = Some(hx(&to[12..]));
                fee_verified = true;
                fee = yokai_fee(&supply, &reserve0, &reserve1, &k_last, to != [0; 32]);
            }
        }
        let effective = &supply + &fee;
        let mut holders = Vec::new();
        let mut total = BigUint::default();

        for chunk in owners.chunks(1024) {
            let keys = chunk
                .iter()
                .map(|(_, addr)| {
                    let mut b = [0u8; 64];
                    b[12..32].copy_from_slice(addr);
                    b[63] = 1;
                    let k: [u8; 32] = Keccak256::digest(b).into();
                    kv(id, &k)
                })
                .collect::<Vec<_>>();
            for ((owner, addr), value) in chunk.iter().zip(db.raw_many(&keys)?) {
                let amount = BigUint::from_bytes_be(&value);
                if amount == BigUint::default() {
                    continue;
                }
                total += &amount;
                holders.push(json!({"account_id":owner,"l2_short_address":hx(addr),"raw_lp":amount.to_string(),
                    "reserve0_based_raw":if fee_verified {Some((&amount*&reserve0/&effective).to_string())}else{None},
                    "reserve1_based_raw":if fee_verified {Some((&amount*&reserve1/&effective).to_string())}else{None},
                    "disposition":if owner.is_none(){"locked_minimum_liquidity_reserved"}else{"receipt_owner_verified_underlying_and_nested_rights_pending"}}));
            }
        }
        ensure!(total <= supply, "Registered LP sum exceeds supply for {id}");
        eprintln!(
            "v0 pair {id}: {} holders; complete known-address supply={}",
            holders.len(),
            total == supply
        );
        pairs.push(json!({"account_id":id,"address":a["l2_short_address"],"factory":factory,"token0":token0,"token1":token1,
            "reserve0":reserve0.to_string(),"reserve1":reserve1.to_string(),"total_supply":supply.to_string(),"k_last":k_last.to_string(),
            "fee_to":fee_to,"fee_calculation_verified":fee_verified,"fee_lp_to_mint":fee.to_string(),"effective_supply":effective.to_string(),
            "known_holders_sum":total.to_string(),"unattributed_supply":(&supply-&total).to_string(),"holders":holders,
            "caveat":"Reserve-based estimates only. burn uses actual token balances, not recorded reserves. Verify wrapper/native units, backing, nested beneficial ownership, transferred-to-pair LP provenance and fee rights before allocation."}));
    }
    Ok(
        json!({"root":db.root(),"runtime_hash":pair_hash,"pairs":pairs,"all_storage_reads_authenticated":true,"scope":"Exact YokaiPair code; registered short addresses plus zero address. Full supply reconciliation is per pool; no payment authorization."}),
    )
}
/// Exact-code WCKB receipt census. Raw v0 msg.value and native balances are shannons;
/// the legacy contract's decimals()=18 is not a conversion of its storage units.
pub fn wckb_holders(path: &Path, inventory: &Path) -> Result<Value> {
    let mut db = V0Db::open(path)?;
    db.rebuild()?;
    let accounts: Vec<Value> = serde_json::from_slice(&fs::read(inventory.join("accounts.json"))?)?;
    let summary: Value = serde_json::from_slice(&fs::read(inventory.join("summary.json"))?)?;
    ensure!(summary["root"] == db.root(), "Inventory root mismatch");
    wckb_holders_from_db(&db, &accounts)
}

/// Shared offline reader: callers supply identities freshly enumerated from this DB.
pub(crate) fn wckb_holders_from_db(db: &V0Db, accounts: &[Value]) -> Result<Value> {
    use num_bigint::BigUint;
    use sha3::{Digest, Keccak256};

    ensure!(
        accounts.len() as u64 == db.tip["account_count"].as_u64().context("count")?
            && accounts
                .iter()
                .enumerate()
                .all(|(i, a)| a["account_id"].as_u64() == Some(i as u64)),
        "Incomplete or noncanonical account inventory"
    );
    let code = "0xeff32b72882b38bb97bde14c7ac09f4cab2ef14f6e089d778a05496f554a2973";
    let mut wrappers = Vec::new();
    for a in accounts.iter().filter(|a| a["runtime_hash"] == code) {
        let id = a["account_id"].as_u64().context("id")? as u32;
        let sh = db.raw(field(id, 2))?;
        ensure!(a["script_hash"] == hx(sh), "Wrapper identity");
        let mut ck = field(id, 255);
        ck[5] = 1;
        ensure!(db.raw(kv(id, &ck))? == hash32(code)?, "Wrapper runtime");
        let backing = db.raw(balance_key(1, &sh[..20]))?;
        ensure!(backing[16..] == [0; 16], "Native width");
        let backing = BigUint::from_bytes_le(&backing);
        let mut total = BigUint::default();
        let mut holders = Vec::new();
        for chunk in accounts.chunks(512) {
            let ids = chunk
                .iter()
                .map(|a| field(a["account_id"].as_u64().unwrap() as u32, 2))
                .collect::<Vec<_>>();
            let shs = db.raw_many(&ids)?;
            let keys = shs
                .iter()
                .map(|sh| {
                    let mut b = [0; 64];
                    b[12..32].copy_from_slice(&sh[..20]);
                    b[63] = 3;
                    let k: [u8; 32] = Keccak256::digest(b).into();
                    kv(id, &k)
                })
                .collect::<Vec<_>>();
            for ((a, sh), v) in chunk.iter().zip(shs).zip(db.raw_many(&keys)?) {
                ensure!(a["script_hash"] == hx(sh), "Owner account identity");
                let amount = BigUint::from_bytes_be(&v);
                if amount == BigUint::default() {
                    continue;
                }
                total += &amount;
                holders.push(json!({"account_id":a["account_id"],"l2_short_address":hx(&sh[..20]),"raw_wckb":amount.to_string(),"underlying_ckb_shannons":amount.to_string(),"disposition":"receipt_census_only_nested_beneficiary_review_required"}));
            }
        }
        let mut zero_key = [0; 64];
        zero_key[63] = 3;
        let zero_key: [u8; 32] = Keccak256::digest(zero_key).into();
        let zero = BigUint::from_bytes_be(&db.raw(kv(id, &zero_key))?);
        total += &zero;
        wrappers.push(json!({"account_id":id,"runtime_hash":code,"native_ckb_shannons":backing.to_string(),"known_holder_sum_shannons":total.to_string(),"zero_address_raw":zero.to_string(),"known_holders_exactly_cover_backing":total==backing,"holders":holders,"coverage_note":"Equality verifies full funded known-holder coverage; otherwise difference is reserved and unknown holder/history discovery remains necessary."}));
    }
    Ok(
        json!({"root":db.root(),"wrappers":wrappers,"all_reads_authenticated":true,"scope":"Same-code WCKB only; 1 raw unit is 1 v0 native shannon despite legacy decimals getter 18. Source body compiled with solc 0.6.6 optimizer 500 runs."}),
    )
}

/// Principal census for the exact historical MasterChef executable body.
/// Rewards remain separate; callers must reconcile principal against each receipt asset.
pub fn farms(path: &Path, inventory: &Path) -> Result<Value> {
    let mut db = V0Db::open(path)?;
    db.rebuild()?;
    let accounts: Vec<Value> = serde_json::from_slice(&fs::read(inventory.join("accounts.json"))?)?;
    let summary: Value = serde_json::from_slice(&fs::read(inventory.join("summary.json"))?)?;
    ensure!(summary["root"] == db.root(), "Inventory root mismatch");
    farms_from_db(&db, &accounts)
}

/// Shared offline reader: callers supply identities freshly enumerated from this DB.
pub(crate) fn farms_from_db(db: &V0Db, accounts: &[Value]) -> Result<Value> {
    use num_bigint::BigUint;
    use sha3::{Digest, Keccak256};

    ensure!(
        accounts.len() as u64 == db.tip["account_count"].as_u64().context("count")?
            && accounts
                .iter()
                .enumerate()
                .all(|(i, a)| a["account_id"].as_u64() == Some(i as u64)),
        "Incomplete inventory"
    );
    let mut owners = Vec::with_capacity(accounts.len());
    for chunk in accounts.chunks(1024) {
        let keys: Vec<_> = chunk
            .iter()
            .map(|a| field(a["account_id"].as_u64().unwrap() as u32, 2))
            .collect();
        for (a, h) in chunk.iter().zip(db.raw_many(&keys)?) {
            ensure!(
                a["script_hash"] == hx(h) && a["l2_short_address"] == hx(&h[..20]),
                "Owner identity"
            );
            owners.push((a["account_id"].as_u64().unwrap(), h));
        }
    }
    let slot = |x: u64| {
        let mut s = [0u8; 32];
        s[24..].copy_from_slice(&x.to_be_bytes());
        s
    };
    let code = "0x40c119039f4d85dab094912217cf608722033bee1824ef4e8d0fdff15e2aa7cb";
    let mut farms = Vec::new();
    for a in accounts.iter().filter(|a| a["runtime_hash"] == code) {
        let id = a["account_id"].as_u64().unwrap() as u32;
        let mut ck = field(id, 255);
        ck[5] = 1;
        ensure!(
            db.raw(kv(id, &ck))? == hash32(code)?,
            "Farm runtime mismatch"
        );
        let header = db.raw_many(&(0..10).map(|i| kv(id, &slot(i))).collect::<Vec<_>>())?;
        ensure!(header[6][..24] == [0; 24], "Pool count overflow");
        let count = u64::from_be_bytes(header[6][24..].try_into()?);
        ensure!(count <= 100, "Pool count bound");
        let base = BigUint::from_bytes_be(&Keccak256::digest(slot(6)));
        let mut pools = Vec::new();
        for pid in 0..count {
            let to_word = |x: BigUint| -> Result<[u8; 32]> {
                let b = x.to_bytes_be();
                ensure!(b.len() <= 32, "Word overflow");
                let mut s = [0; 32];
                s[32 - b.len()..].copy_from_slice(&b);
                Ok(s)
            };
            let keys = (0u64..5)
                .map(|i| Ok(kv(id, &to_word(&base + pid * 5 + i)?)))
                .collect::<Result<Vec<_>>>()?;
            let state = db.raw_many(&keys)?;
            let mut outer = [0; 64];
            outer[..32].copy_from_slice(&slot(pid));
            outer[32..].copy_from_slice(&slot(7));
            let outer: [u8; 32] = Keccak256::digest(outer).into();
            let mut total = BigUint::default();
            let mut users = Vec::new();
            for chunk in owners.chunks(1024) {
                let slots: Vec<[u8; 32]> = chunk
                    .iter()
                    .map(|(_, h)| {
                        let mut b = [0; 64];
                        b[12..32].copy_from_slice(&h[..20]);
                        b[32..].copy_from_slice(&outer);
                        Keccak256::digest(b).into()
                    })
                    .collect();
                let keys: Vec<_> = slots.iter().map(|s| kv(id, s)).collect();
                for (((owner, h), key), value) in
                    chunk.iter().zip(slots.iter()).zip(db.raw_many(&keys)?)
                {
                    let amount = BigUint::from_bytes_be(&value);
                    if amount == BigUint::default() {
                        continue;
                    }
                    total += &amount;
                    let reward_key = to_word(BigUint::from_bytes_be(key) + 1u8)?;
                    let debt = BigUint::from_bytes_be(&db.raw(kv(id, &reward_key))?);
                    users.push(json!({"account_id":owner,"l2_short_address":hx(&h[..20]),"principal_raw":amount.to_string(),"reward_debt_raw":debt.to_string()}));
                }
            }
            eprintln!(
                "v0 farm {id} pool {pid}: {} positive principals",
                users.len()
            );
            pools.push(json!({"pid":pid,"lp_token":hx(&state[0][12..]),"alloc_point":BigUint::from_bytes_be(&state[1]).to_string(),"last_reward_time":BigUint::from_bytes_be(&state[2]).to_string(),"acc_yok_per_share":BigUint::from_bytes_be(&state[3]).to_string(),"staking_rewards":hx(&state[4][12..]),"known_principal_sum_raw":total.to_string(),"users":users}));
        }
        farms.push(json!({"account_id":id,"owner_short_address":hx(&header[0][12..]),"yok_short_address":hx(&header[1][12..]),"monster_short_address":hx(&header[2][12..]),"yok_per_second":BigUint::from_bytes_be(&header[3]).to_string(),"pool_count":count,"pools":pools}));
    }
    Ok(
        json!({"root":db.root(),"runtime_hash":code,"all_reads_authenticated":true,"farms":farms,"scope":"All registered short addresses. Principal only; receipt custody, unknown users and nested beneficiaries require reconciliation. Independent rewards excluded by policy."}),
    )
}
/// Discover unknown WCKB/LP holders and provenance of retained native balances.
/// Receipt history is a discovery aid; current balances are authenticated at the root.
pub fn beneficiary_history(path: &Path, inventory: &Path) -> Result<Value> {
    let mut db = V0Db::open(path)?;
    db.rebuild()?;
    let accounts: Vec<Value> = serde_json::from_slice(&fs::read(inventory.join("accounts.json"))?)?;
    beneficiary_history_from_db(&db, &accounts)
}

/// Shared offline reader: callers supply identities freshly enumerated from this DB.
pub(crate) fn beneficiary_history_from_db(db: &V0Db, accounts: &[Value]) -> Result<Value> {
    use num_bigint::BigUint;
    use sha3::{Digest, Keccak256};
    use std::collections::{BTreeMap, BTreeSet};

    ensure!(
        accounts.len() as u64 == db.tip["account_count"].as_u64().context("count")?,
        "count"
    );
    let mut known = BTreeSet::new();
    for chunk in accounts.chunks(512) {
        let keys = chunk
            .iter()
            .map(|a| field(a["account_id"].as_u64().unwrap() as u32, 2))
            .collect::<Vec<_>>();
        for (a, sh) in chunk.iter().zip(db.raw_many(&keys)?) {
            ensure!(a["script_hash"] == hx(sh), "identity");
            known.insert(sh[..20].to_vec());
        }
    }
    let mut contracts = BTreeMap::<Vec<u8>, (u32, u8)>::new();
    for a in accounts {
        let slot = match a["runtime_hash"].as_str() {
            Some("0xeff32b72882b38bb97bde14c7ac09f4cab2ef14f6e089d778a05496f554a2973") => Some(3),
            Some("0xf601f6484fc84f0515c0ed834594c582a3974f00e4dc030ec489e8b57c75f8da") => Some(1),
            _ => None,
        };
        if let Some(slot) = slot {
            contracts.insert(
                bytes(a["l2_short_address"].as_str().unwrap())?,
                (a["account_id"].as_u64().unwrap() as u32, slot),
            );
        }
    }
    let targets: [u64; 8] = [12119, 12134, 28983, 37790, 37800, 49358, 36838, 23982];
    let targets: BTreeSet<_> = accounts
        .iter()
        .filter(|a| targets.contains(&a["account_id"].as_u64().unwrap()))
        .map(|a| bytes(a["l2_short_address"].as_str().unwrap()).unwrap())
        .collect();
    let mut native = BTreeMap::<(u32, u8, Vec<u8>, Vec<u8>), (BigUint, u64, String)>::new();
    let mut candidates = BTreeSet::new();
    let native_ids = accounts
        .iter()
        .filter(|a| a["kind"] == "native_sudt")
        .map(|a| a["account_id"].as_u64().unwrap() as u32)
        .collect::<BTreeSet<_>>();
    let mut native_candidates = BTreeSet::new();
    let topic: [u8; 32] = Keccak256::digest(b"Transfer(address,address,uint256)").into();
    let store = db.tree.store();
    let cf = store.db.cf_handle("6").context("receipts")?;
    let mut opts = rocksdb::ReadOptions::default();
    opts.fill_cache(false);
    let mut count = 0u64;
    for item in store
        .db
        .iterator_cf_opt(cf, opts, rocksdb::IteratorMode::Start)
    {
        let (k, v) = item?;
        let f = table(&v, 4)?;
        let raw = f[3];
        if raw.len() > 4 {
            let first = u32le(&raw[4..8])? as usize;
            for item in table(raw, first / 4 - 1)? {
                let l = table(item, 3)?;
                let asset = u32le(l[0])?;
                ensure!(l[1].len() == 1, "flag");
                let flag = l[1][0];
                let data = l[2];
                ensure!(
                    data.len() >= 4 && u32le(&data[..4])? as usize == data.len() - 4,
                    "bytes"
                );
                let b = &data[4..];
                if flag <= 1 {
                    ensure!(!b.is_empty(), "native bytes");
                    let len = b[0] as usize;
                    ensure!(len <= 32 && b.len() == 1 + 2 * len + 16, "native log");
                    let from = &b[1..1 + len];
                    let to = &b[1 + len..1 + 2 * len];
                    if native_ids.contains(&asset) {
                        for address in [from, to] {
                            if !known.contains(address) {
                                native_candidates.insert((asset, address.to_vec()));
                            }
                        }
                    }
                    if targets.contains(from) || targets.contains(to) || !known.contains(to) {
                        let value = BigUint::from_bytes_le(&b[1 + 2 * len..]);
                        let e = native
                            .entry((asset, flag, from.to_vec(), to.to_vec()))
                            .or_insert((BigUint::default(), 0, hx(&k)));
                        e.0 += value;
                        e.1 += 1;
                    }
                } else if flag == 3 {
                    ensure!(b.len() >= 28, "EVM log");
                    if let Some(&(id, slot)) = contracts.get(&b[..20]) {
                        let size = u32le(&b[20..24])? as usize;
                        ensure!(24 + size + 4 <= b.len(), "EVM data");
                        let n = u32le(&b[24 + size..28 + size])? as usize;
                        ensure!(28 + size + 32 * n == b.len(), "EVM topics");
                        let topics = &b[28 + size..];
                        if n == 3 && topics[..32] == topic {
                            for a in [&topics[44..64], &topics[76..96]] {
                                if !known.contains(a) && a != [0u8; 20] {
                                    candidates.insert((id, slot, a.to_vec()));
                                }
                            }
                        }
                    }
                }
            }
        }
        count += 1;
        if count % 1000000 == 0 {
            eprintln!(
                "beneficiary history {count}; candidates {}",
                candidates.len()
            );
        }
    }
    let native_candidates = native_candidates.into_iter().collect::<Vec<_>>();
    let mut unknown_native = Vec::new();
    for chunk in native_candidates.chunks(512) {
        let keys = chunk
            .iter()
            .map(|(asset, address)| balance_key(*asset, address))
            .collect::<Vec<_>>();
        for ((asset, address), value) in chunk.iter().zip(db.raw_many(&keys)?) {
            ensure!(
                value[16..] == [0; 16],
                "Noncanonical unmapped native balance"
            );
            let amount = BigUint::from_bytes_le(&value[..16]);
            if amount > BigUint::default() {
                unknown_native.push(json!({"asset_account_id":asset,"short_address":hx(address),"raw_amount":amount.to_string(),"disposition":"unregistered_owner_retained"}));
            }
        }
    }
    let mut unknown = Vec::new();
    for (id, slot, addr) in candidates {
        let mut k = [0; 64];
        k[12..32].copy_from_slice(&addr);
        k[63] = slot;
        let hash: [u8; 32] = Keccak256::digest(k).into();
        let v = BigUint::from_bytes_be(&db.raw(kv(id, &hash))?);
        if v > BigUint::default() {
            unknown.push(json!({"contract_account_id":id,"balance_slot":slot,"unknown_short_address":hx(addr),"current_raw":v.to_string(),"status":"reserved_no_verified_ETH_owner"}));
        }
    }
    Ok(
        json!({"root":db.root(),"receipts_scanned":count,"current_receipt_balances_authenticated":true,"history_is_discovery_not_complete_ownership_proof":true,"unknown_receipt_holders":unknown,"unknown_native_holders":unknown_native,"unknown_native_candidates_checked":native_candidates.len(),"native_transfer_edges":native.into_iter().map(|((id,flag,from,to),(value,count,receipt))|json!({"asset_account_id":id,"service_flag":flag,"from":hx(from),"to":hx(to),"total_raw":value.to_string(),"logs":count,"example_receipt_key":receipt})).collect::<Vec<_>>()}),
    )
}
