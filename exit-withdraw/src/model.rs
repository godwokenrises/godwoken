use anyhow::{bail, ensure, Context, Result};
use gw_types::{bytes::Bytes, packed, prelude::*};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const ROLLUP_HASH: &str = "0x1ca35cb5fda4bd542e71d94a6d5f4c0d255d6d6fba73c41cf45d2693e59b3072";
pub const FAMILIES: [&str; 5] = ["custodian", "deposit", "withdrawal", "stake", "challenge"];
pub const NATIVE: &str = "0x0000000000000000000000000000000000000000000000000000000000000000";
pub fn hx(b: impl AsRef<[u8]>) -> String {
    format!("0x{}", hex::encode(b))
}
pub fn bytes(s: &str) -> Result<Vec<u8>> {
    let raw = s.strip_prefix("0x").context("Expected 0x-prefixed bytes")?;
    Ok(hex::decode(raw).context("Invalid hex")?)
}
pub fn hash32(s: &str) -> Result<[u8; 32]> {
    bytes(s)?
        .try_into()
        .map_err(|_| anyhow::anyhow!("Expected 32 bytes"))
}
pub fn quantity(s: &str) -> Result<u64> {
    Ok(u64::from_str_radix(
        s.strip_prefix("0x").context("Expected hex quantity")?,
        16,
    )?)
}
pub fn hash(b: &[u8]) -> [u8; 32] {
    gw_hash::blake2b::hash(b)
}
pub fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}
pub fn eth_address(s: &str) -> Result<[u8; 20]> {
    bytes(s)?
        .try_into()
        .map_err(|_| anyhow::anyhow!("ETH address must contain 20 bytes"))
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Script {
    pub code_hash: String,
    pub hash_type: String,
    pub args: String,
}
impl Script {
    pub fn packed(&self) -> Result<packed::Script> {
        let kind: u8 = match self.hash_type.as_str() {
            "data" => 0,
            "type" => 1,
            "data1" => 2,
            "data2" => 4,
            _ => bail!("Unknown Script hash_type"),
        };
        let args: Bytes = bytes(&self.args)?.into();
        Ok(packed::Script::new_builder()
            .code_hash(hash32(&self.code_hash)?.pack())
            .hash_type(kind.into())
            .args(args.pack())
            .build())
    }
    pub fn from_packed(s: packed::Script) -> Result<Self> {
        let kind: u8 = s.hash_type().into();
        let kind = match kind {
            0 => "data",
            1 => "type",
            2 => "data1",
            4 => "data2",
            _ => bail!("Unsupported Script hash_type"),
        };
        Ok(Self {
            code_hash: hx(s.code_hash().as_slice()),
            hash_type: kind.into(),
            args: hx(s.args().raw_data()),
        })
    }
    pub fn hash(&self) -> Result<String> {
        Ok(hx(self.packed()?.hash()))
    }
}
pub fn rollup_script() -> Script {
    Script {
        code_hash: "0xfef1d086d9f74d143c60bf03bd04bab29200dbf484c801c72774f2056d4c6718".into(),
        hash_type: "type".into(),
        args: "0xab21bfe2bf85927bb42faaf3006a355222e24d5ea1d4dec0e62f53a8e0c04690".into(),
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct OutPoint {
    pub tx_hash: String,
    pub index: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct CellOutput {
    pub capacity: String,
    pub lock: Script,
    #[serde(rename = "type")]
    pub type_script: Option<Script>,
}
impl CellOutput {
    pub fn packed(&self) -> Result<packed::CellOutput> {
        Ok(packed::CellOutput::new_builder()
            .capacity(quantity(&self.capacity)?.pack())
            .lock(self.lock.packed()?)
            .type_(
                self.type_script
                    .as_ref()
                    .map(Script::packed)
                    .transpose()?
                    .pack(),
            )
            .build())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Cell {
    pub out_point: OutPoint,
    pub output: CellOutput,
    pub output_data: String,
    #[serde(default)]
    pub block_number: String,
    #[serde(default)]
    pub tx_index: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Bundle {
    pub schema_version: u32,
    pub network: String,
    pub observed_start: String,
    pub observed_end: String,
    pub rollup_cell: Cell,
    pub config_data: String,
    pub cells: BTreeMap<String, Vec<Cell>>,
    #[serde(default)]
    pub consistency: String,
    #[serde(default)]
    pub live_cells_checked: usize,
    #[serde(default)]
    pub ckb_rpc: Option<String>,
    #[serde(default)]
    pub indexer_rpc: Option<String>,
    #[serde(default)]
    pub indexer_before: serde_json::Value,
    #[serde(default)]
    pub indexer_after: serde_json::Value,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GlobalState {
    pub config_hash: String,
    pub account_root: String,
    pub account_count: u32,
    pub block_count: u64,
    pub block_root: String,
    pub reverted_root: String,
    pub tip_hash: String,
    pub timestamp: u64,
    pub finalized_timepoint: u64,
    pub status: u8,
    pub version: u8,
}
impl GlobalState {
    pub fn parse(data: &[u8]) -> Result<Self> {
        let s = packed::GlobalState::from_slice(data).context("Invalid v1 GlobalState")?;
        let result = Self {
            config_hash: hx(s.rollup_config_hash().as_slice()),
            account_root: hx(s.account().merkle_root().as_slice()),
            account_count: s.account().count().unpack(),
            block_count: s.block().count().unpack(),
            block_root: hx(s.block().merkle_root().as_slice()),
            reverted_root: hx(s.reverted_block_root().as_slice()),
            tip_hash: hx(s.tip_block_hash().as_slice()),
            timestamp: s.tip_block_timestamp().unpack(),
            finalized_timepoint: s.last_finalized_timepoint().unpack(),
            status: s.status().into(),
            version: s.version().into(),
        };
        ensure!(
            result.block_count > 0 && matches!(result.version, 1 | 2) && result.status <= 1,
            "Unsupported GlobalState version/status/count"
        );
        Ok(result)
    }
}
#[derive(Clone)]
pub struct Config {
    pub packed: packed::RollupConfig,
    pub hashes: BTreeMap<String, String>,
    pub finality_blocks: u64,
}
impl Config {
    pub fn parse(data: &[u8]) -> Result<Self> {
        let c = packed::RollupConfig::from_slice(data).context("Invalid RollupConfig")?;
        let chain_id: u64 = c.chain_id().unpack();
        ensure!(chain_id == 71402, "Not mainnet v1 config");
        let hashes = [
            ("l1_sudt", c.l1_sudt_script_type_hash()),
            ("l2_sudt", c.l2_sudt_validator_script_type_hash()),
            ("custodian", c.custodian_script_type_hash()),
            ("deposit", c.deposit_script_type_hash()),
            ("withdrawal", c.withdrawal_script_type_hash()),
            ("stake", c.stake_script_type_hash()),
            ("challenge", c.challenge_script_type_hash()),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_owned(), hx(v.as_slice())))
        .collect();
        Ok(Self {
            finality_blocks: c.finality_blocks().unpack(),
            packed: c,
            hashes,
        })
    }
}
impl Bundle {
    pub fn validate(&self) -> Result<(GlobalState, Config)> {
        ensure!(
            self.schema_version == 1 && self.network == "mainnet_v1",
            "Wrong observation version/network"
        );
        ensure!(
            self.rollup_cell.output.type_script.as_ref() == Some(&rollup_script())
                && rollup_script().hash()? == ROLLUP_HASH,
            "Wrong rollup identity"
        );
        let state = GlobalState::parse(&bytes(&self.rollup_cell.output_data)?)?;
        let config_data = bytes(&self.config_data)?;
        ensure!(
            hx(hash(&config_data)) == state.config_hash,
            "Config hash does not match rollup commitment"
        );
        let cfg = Config::parse(&config_data)?;
        ensure!(
            self.cells.len() == FAMILIES.len()
                && FAMILIES.iter().all(|f| self.cells.contains_key(*f)),
            "Incomplete L1 cell families"
        );
        let mut seen = std::collections::BTreeSet::new();
        for (family, cells) in &self.cells {
            for cell in cells {
                hash32(&cell.out_point.tx_hash)?;
                ensure!(
                    quantity(&cell.out_point.index)? <= u32::MAX as u64,
                    "Invalid output index"
                );
                let l = &cell.output.lock;
                ensure!(
                    l.code_hash == cfg.hashes[family]
                        && l.hash_type == "type"
                        && bytes(&l.args)?.starts_with(&hash32(ROLLUP_HASH)?),
                    "Wrong cell family or rollup"
                );
                ensure!(seen.insert(cell.out_point.clone()), "Duplicate L1 outpoint");
            }
        }
        Ok((state, cfg))
    }
}
#[derive(Clone, Debug, Serialize)]
pub struct Tip {
    pub hash: String,
    pub number: u64,
    pub account_root: String,
    pub account_count: u32,
    pub timestamp: u64,
}
impl Tip {
    pub fn from_block(b: &packed::L2Block) -> Self {
        let raw = b.raw();
        Self {
            hash: hx(hash(raw.as_slice())),
            number: raw.number().unpack(),
            account_root: hx(raw.post_account().merkle_root().as_slice()),
            account_count: raw.post_account().count().unpack(),
            timestamp: raw.timestamp().unpack(),
        }
    }
}
