use super::*;
use anyhow::{ensure, Context, Result};
use gw_types::U256;
use std::collections::BTreeMap;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged, deny_unknown_fields)]
pub enum Request {
    Address(String),
    Amounts {
        eth_address: String,
        #[serde(deserialize_with = "asset_amounts")]
        amounts: BTreeMap<u32, String>,
    },
}
fn asset_amounts<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> std::result::Result<BTreeMap<u32, String>, D::Error> {
    let values = BTreeMap::<String, String>::deserialize(d)?;
    let mut result = BTreeMap::new();
    for (id, value) in values {
        let n = id.parse::<u32>().map_err(serde::de::Error::custom)?;
        if id != n.to_string() {
            return Err(serde::de::Error::custom(
                "Asset IDs must be canonical decimal integers",
            ));
        }
        result.insert(n, value);
    }
    Ok(result)
}
impl Request {
    pub(crate) fn address(&self) -> Result<[u8; 20]> {
        eth_address(match self {
            Self::Address(s) => s,
            Self::Amounts { eth_address, .. } => eth_address,
        })
    }
}
fn settlement_version<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> std::result::Result<u32, D::Error> {
    let version = u32::deserialize(d)?;
    if version != exit_withdraw_state_validator::settlement::FORMAT_VERSION {
        return Err(serde::de::Error::custom("Expected V3 settlement skeleton"));
    }
    Ok(version)
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Skeleton {
    #[serde(deserialize_with = "settlement_version")]
    pub schema_version: u32,
    pub status: String,
    pub transaction: Transaction,
    pub transaction_hash: String,
    pub input_cells: Vec<Cell>,
    pub dependency_cells: Vec<Cell>,
    pub before_root: String,
    pub after_root: String,
    pub profile_hash: String,
    pub fee_shannons: u64,
    pub allocations: serde_json::Value,
    pub signing_groups: serde_json::Value,
    pub journal: serde_json::Value,
}
pub(crate) fn add(
    map: &mut BTreeMap<contract::Hash, U256>,
    key: contract::Hash,
    n: U256,
) -> Result<()> {
    let v = map.entry(key).or_default();
    *v = v.checked_add(n).context("Amount overflow")?;
    Ok(())
}
pub(crate) fn token(
    c: &contract::Cell,
    cfg: &packed::RollupConfig,
) -> Result<Option<(contract::Hash, U256)>> {
    if let Some(t) = &c.type_ {
        ensure!(
            t.code_hash().as_slice() == cfg.l1_sudt_script_type_hash().as_slice()
                && u8::from(t.hash_type()) == 1
                && t.args().raw_data().len() == 32
                && c.data.len() == 16,
            "Unsupported custodian token"
        );
        Ok(Some((
            contract::script_hash(t),
            U256::from_little_endian(&c.data),
        )))
    } else {
        ensure!(c.data.is_empty(), "Untyped custodian carries data");
        Ok(None)
    }
}
pub(crate) fn change_cells(
    selected: &BTreeMap<contract::Hash, U256>,
    needed: &BTreeMap<contract::Hash, U256>,
    scripts: &BTreeMap<contract::Hash, packed::Script>,
    p: &contract::Profile,
) -> Result<Vec<contract::Cell>> {
    let mut out = Vec::new();
    for (h, n) in selected {
        if *h == [0; 32] {
            continue;
        }
        let remaining = n
            .checked_sub(*needed.get(h).unwrap_or(&U256::zero()))
            .context("Insufficient selected token")?;
        let mut remainder = remaining;
        let mut preserve_zero_type = remainder.is_zero() && !needed.contains_key(h);
        while !remainder.is_zero() || preserve_zero_type {
            preserve_zero_type = false;
            let quantity = remainder.min(U256::from(u128::MAX));
            let mut cell = contract::Cell {
                capacity: 0,
                lock: p.change.clone(),
                type_: Some(
                    scripts
                        .get(h)
                        .context("Missing change type preimage")?
                        .clone(),
                ),
                data: quantity.low_u128().to_le_bytes().to_vec(),
            };
            cell.capacity = cell
                .minimum_capacity()
                .map_err(|e| anyhow::anyhow!("Capacity: {e:?}"))?;
            out.push(cell);
            remainder -= quantity;
            ensure!(
                out.len() <= contract::MAX_OUTPUTS,
                "Too many change cells; split request"
            );
        }
    }
    if out.is_empty() {
        let mut c = contract::Cell {
            capacity: 0,
            lock: p.change.clone(),
            type_: None,
            data: vec![],
        };
        c.capacity = c
            .minimum_capacity()
            .map_err(|e| anyhow::anyhow!("Capacity: {e:?}"))?;
        out.push(c);
    }
    Ok(out)
}
