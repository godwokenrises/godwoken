//! ReadOnly RocksDB adapter; Godwoken owns the key encodings and SMT proof logic.
use crate::{model::*, schema::*, store_serde};
use anyhow::{ensure, Context, Result};
use gw_common::{error::Error as StateError, state::State};
use gw_smt::{
    smt::{Blake2bHasher, SMT},
    sparse_merkle_tree::{
        error::Error as SmtError, traits::StoreReadOps, BranchKey, BranchNode, H256 as SmtH256,
    },
};
use gw_types::{h256::H256, packed, prelude::*, registry_address::RegistryAddress};
use rocksdb::{Options, DB};
use std::{
    cell::Cell,
    path::{Path, PathBuf},
    sync::Arc,
};

pub struct ReadStore {
    pub(crate) db: Arc<DB>,
}
impl ReadStore {
    pub(crate) fn get(&self, col: usize, key: &[u8]) -> Result<Option<Vec<u8>>> {
        let cf = self
            .db
            .cf_handle(&col.to_string())
            .context("Missing Godwoken column family")?;
        Ok(self.db.get_cf(cf, key)?)
    }
}
impl StoreReadOps<SmtH256> for ReadStore {
    fn get_branch(&self, key: &BranchKey) -> std::result::Result<Option<BranchNode>, SmtError> {
        let data = self
            .get(
                COLUMN_ACCOUNT_SMT_BRANCH,
                &store_serde::branch_key_to_vec(key),
            )
            .map_err(|e| SmtError::Store(e.to_string()))?;
        data.map(|b| {
            // The upstream decoder expects trusted DB bytes and panics on malformed input.
            // Validate discriminator/length before delegating to it.
            let expected = match b.first() {
                Some(0) => 65,
                Some(1 | 2 | 4 | 5) => 98,
                Some(3 | 6 | 7 | 8) => 131,
                _ => return Err(SmtError::Store("Unknown SMT branch encoding".into())),
            };
            if b.len() != expected {
                return Err(SmtError::Store("Malformed SMT branch length".into()));
            }
            #[cfg(not(feature = "smt-trie"))]
            if b[0] >= 4 {
                return Err(SmtError::Store("Trie DB requires smt-trie feature".into()));
            }
            Ok(store_serde::slice_to_branch_node(&b))
        })
        .transpose()
    }
    fn get_leaf(&self, key: &SmtH256) -> std::result::Result<Option<SmtH256>, SmtError> {
        self.get(COLUMN_ACCOUNT_SMT_LEAF, key.as_slice())
            .map_err(|e| SmtError::Store(e.to_string()))?
            .map(|b| {
                <[u8; 32]>::try_from(b.as_slice())
                    .map(SmtH256::from)
                    .map_err(|_| SmtError::Store("Invalid SMT leaf length".into()))
            })
            .transpose()
    }
}

pub struct Database {
    pub path: PathBuf,
    pub tip: Tip,
    pub tree: SMT<ReadStore>,
    pub chain_id: [u8; 32],
    pub schema_version: Option<String>,
    pub checked_reads: Cell<u64>,
}
impl Database {
    pub fn open(path: &Path) -> Result<Self> {
        ensure!(
            path.join("CURRENT").is_file(),
            "--db must be an existing RocksDB directory containing CURRENT"
        );
        let path = path.canonicalize()?;
        let mut options = Options::default();
        options.create_if_missing(false);
        let names = DB::list_cf(&options, &path).context("Read RocksDB column families")?;
        for n in [
            COLUMN_META,
            COLUMN_BLOCK,
            COLUMN_ACCOUNT_SMT_BRANCH,
            COLUMN_ACCOUNT_SMT_LEAF,
            COLUMN_SCRIPT,
            COLUMN_ASSET_SCRIPT,
        ] {
            ensure!(
                names.contains(&n.to_string()),
                "Required Godwoken CF {n} missing"
            );
        }
        let db = Arc::new(
            DB::open_cf_for_read_only(&options, &path, names, false)
                .context("Open Godwoken DB read-only (no migration)")?,
        );
        let store = ReadStore { db };
        let chain_id: [u8; 32] = store
            .get(COLUMN_META, META_CHAIN_ID_KEY)?
            .context("Missing CHAIN_ID")?
            .try_into()
            .map_err(|_| anyhow::anyhow!("Invalid CHAIN_ID"))?;
        ensure!(hx(chain_id) == ROLLUP_HASH, "DB belongs to another rollup");
        let tip_hash = store
            .get(COLUMN_META, META_LAST_VALID_TIP_BLOCK_HASH_KEY)?
            .context("Missing last-valid tip")?;
        ensure!(tip_hash.len() == 32, "Invalid tip hash");
        let block = store
            .get(COLUMN_BLOCK, &tip_hash)?
            .context("Missing last-valid block")?;
        let block = packed::L2Block::from_slice(&block).context("Invalid DB L2Block")?;
        let tip = Tip::from_block(&block);
        ensure!(tip.hash == hx(&tip_hash), "DB block hash/preimage mismatch");
        ensure!(
            tip.account_count <= 10_000_000,
            "Unreasonable account count"
        );
        let schema_version = store.get(COLUMN_META, MIGRATION_VERSION_KEY)?.map(hx);
        let tree = SMT::new_with_store(store).context("Derive root from stored SMT root branch")?;
        Ok(Self {
            path,
            tip,
            tree,
            chain_id,
            schema_version,
            checked_reads: Cell::new(0),
        })
    }
    /// Authenticate a batch against the same stored root. Preserve caller key order.
    /// Duplicate keys are rejected rather than handed to the SMT proof builder.
    pub fn raw_many(&self, keys: &[[u8; 32]]) -> Result<Vec<[u8; 32]>> {
        ensure!(
            keys.iter().collect::<std::collections::BTreeSet<_>>().len() == keys.len(),
            "Duplicate batch key"
        );
        let mut result = Vec::with_capacity(keys.len());
        for chunk in keys.chunks(1024) {
            let keys = chunk.iter().copied().map(SmtH256::from).collect::<Vec<_>>();
            let leaves = keys
                .iter()
                .map(|k| Ok((*k, self.tree.store().get_leaf(k)?.unwrap_or_default())))
                .collect::<Result<Vec<_>>>()?;
            let proof = self.tree.merkle_proof(keys)?;
            ensure!(
                proof.verify::<Blake2bHasher>(self.tree.root(), leaves.clone())?,
                "Batch SMT proof failed"
            );
            result.extend(leaves.into_iter().map(|(_, v)| <[u8; 32]>::from(v)));
            self.checked_reads
                .set(self.checked_reads.get() + chunk.len() as u64);
        }
        Ok(result)
    }
    pub fn root(&self) -> String {
        hx(self.tree.root().as_slice())
    }
    pub fn script(&self, sh: &H256, asset: bool) -> Result<Option<Script>> {
        self.tree
            .store()
            .get(
                if asset {
                    COLUMN_ASSET_SCRIPT
                } else {
                    COLUMN_SCRIPT
                },
                sh,
            )?
            .map(|b| {
                ensure!(hash(&b) == *sh, "Script preimage/hash mismatch");
                Script::from_packed(
                    packed::Script::from_slice(&b).context("Invalid Script encoding")?,
                )
            })
            .transpose()
    }
    /// Read a content-addressed runtime-code preimage whose hash came from authenticated state.
    pub fn data(&self, hash_value: &H256) -> Result<Option<Vec<u8>>> {
        let data = self.tree.store().get(COLUMN_DATA, hash_value)?;
        if let Some(ref bytes) = data {
            ensure!(hash(bytes) == *hash_value, "Data preimage/hash mismatch");
        }
        Ok(data)
    }
    pub fn account(&self, id: u32) -> Result<(H256, Script)> {
        let sh = self.get_script_hash(id)?;
        ensure!(sh != [0; 32], "Missing Script hash for account {id}");
        Ok((
            sh,
            self.script(&sh, false)?
                .context("Missing account Script preimage")?,
        ))
    }
    pub fn registry(&self, sh: &H256) -> Result<Option<RegistryAddress>> {
        let raw = self.get_value(
            2,
            &gw_common::state::build_script_hash_to_registry_address_key(sh),
        )?;
        if raw == [0; 32] {
            return Ok(None);
        }
        ensure!(raw[28..] == [0; 4], "Registry padding is nonzero");
        let addr = RegistryAddress::from_slice(&raw).context("Invalid registry encoding")?;
        ensure!(
            addr.registry_id == 2 && addr.address.len() == 20,
            "Unsupported registry address"
        );
        ensure!(
            self.get_script_hash_by_registry_address(&addr)? == Some(*sh),
            "Registry reverse mapping mismatch"
        );
        Ok(Some(addr))
    }
    pub fn lookup_eth(&self, eth: &[u8; 20]) -> Result<serde_json::Value> {
        let addr = RegistryAddress::new(2, eth.to_vec());
        let Some(sh) = self.get_script_hash_by_registry_address(&addr)? else {
            return Ok(
                serde_json::json!({"status":"unregistered","registry_address":hx(addr.to_bytes())}),
            );
        };
        let key = gw_common::state::build_script_hash_to_account_id_key(&sh);
        let encoded = self.get_raw(&key)?;
        ensure!(
            encoded[4] == 1 && encoded[5..] == [0; 27],
            "Invalid account-ID mapping"
        );
        let id = u32::from_le_bytes(encoded[..4].try_into()?);
        ensure!(
            id < self.tip.account_count && self.get_script_hash(id)? == sh,
            "Registry/account mapping mismatch"
        );
        ensure!(
            self.registry(&sh)? == Some(addr.clone()),
            "Registry round-trip mismatch"
        );
        let script = self.script(&sh, false)?.context("Missing address Script")?;
        ensure!(
            bytes(&script.args)?.starts_with(&self.chain_id),
            "Address Script has wrong rollup prefix"
        );
        Ok(
            serde_json::json!({"status":"registered","registry_address":hx(addr.to_bytes()),"account_id":id,"script_hash":hx(sh),"script":script}),
        )
    }
}
impl State for Database {
    fn get_raw(&self, key: &H256) -> std::result::Result<H256, StateError> {
        let checked = || -> Result<H256> {
            let key: SmtH256 = (*key).into();
            // Read the column even for an empty tree so an inconsistent stray leaf fails.
            let val = self.tree.store().get_leaf(&key)?.unwrap_or_default();
            let proof = self.tree.merkle_proof(vec![key])?;
            ensure!(
                proof.verify::<Blake2bHasher>(self.tree.root(), vec![(key, val)])?,
                "SMT proof failed for {}",
                hx(key.as_slice())
            );
            self.checked_reads.set(self.checked_reads.get() + 1);
            Ok(val.into())
        };
        checked().map_err(|e| StateError::SMT(e.to_string()))
    }
    fn get_account_count(&self) -> std::result::Result<u32, StateError> {
        Ok(self.tip.account_count)
    }
    fn calculate_root(&self) -> std::result::Result<H256, StateError> {
        Ok((*self.tree.root()).into())
    }
    fn update_raw(&mut self, _: H256, _: H256) -> std::result::Result<(), StateError> {
        Err(StateError::Store)
    }
    fn set_account_count(&mut self, _: u32) -> std::result::Result<(), StateError> {
        Err(StateError::Store)
    }
}
