//! Shared transaction models and chain utilities for V3 settlement and L1 wallet claims.
pub mod build;
pub mod chain;
pub mod state;
use crate::model::*;
use anyhow::{ensure, Context, Result};
use ckb_types::prelude::Entity as CkbEntity;
use exit_withdraw_state_validator as contract;
use gw_types::{packed, prelude::*};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Deployment {
    pub profile: String,
    pub profile_hash: String,
    pub validator_data_hash: String,
    pub cell_deps: Vec<Dependency>,
    pub sponsor_inputs: Vec<OutPoint>,
    /// Complete legacy multisig preimage: reserved, require_first_n, threshold, count, pubkey hashes.
    pub multisig_config: String,
    /// Required only for the bootstrap rollup input's lock signature placeholder.
    pub bootstrap_lock_placeholder: Option<String>,
    pub fee_shannons: u64,
    pub confirmations: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Dependency {
    pub out_point: OutPoint,
    pub dep_type: String,
    pub data_hash: String,
}
impl Deployment {
    pub fn multisig_placeholder(&self, p: &contract::Profile) -> Result<Vec<u8>> {
        let c = bytes(&self.multisig_config)?;
        ensure!(
            c.len() >= 4
                && c[0] == 0
                && c[2] > 0
                && c[1] <= c[2]
                && c[2] <= c[3]
                && c.len() == 4 + 20 * c[3] as usize,
            "Invalid multisig config"
        );
        ensure!(
            p.governance.args().raw_data().as_ref() == &hash(&c)[..20],
            "Multisig config does not match governance lock"
        );
        let mut placeholder = c.clone();
        placeholder.resize(c.len() + 65 * c[2] as usize, 0);
        Ok(placeholder)
    }
}
pub fn outpoint(p: &OutPoint) -> Result<packed::OutPoint> {
    Ok(packed::OutPoint::new_builder()
        .tx_hash(hash32(&p.tx_hash)?.pack())
        .index(u32::try_from(quantity(&p.index)?)?.pack())
        .build())
}
pub fn from_outpoint(p: &packed::OutPoint) -> OutPoint {
    OutPoint {
        tx_hash: hx(p.tx_hash().as_slice()),
        index: format!(
            "0x{:x}",
            u32::from_le_bytes(p.index().as_slice().try_into().unwrap())
        ),
    }
}
pub fn contract_cell(c: &Cell) -> Result<contract::Cell> {
    output_cell(&c.output, &c.output_data)
}
pub fn output_cell(output: &CellOutput, data: &str) -> Result<contract::Cell> {
    Ok(contract::Cell {
        capacity: quantity(&output.capacity)?,
        lock: output.lock.packed()?,
        type_: output
            .type_script
            .as_ref()
            .map(Script::packed)
            .transpose()?,
        data: bytes(data)?,
    })
}
pub fn cell_output(c: &contract::Cell) -> Result<CellOutput> {
    Ok(CellOutput {
        capacity: format!("0x{:x}", c.capacity),
        lock: Script::from_packed(c.lock.clone())?,
        type_script: c
            .type_
            .as_ref()
            .map(|s| Script::from_packed(s.clone()))
            .transpose()?,
    })
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Transaction {
    pub version: String,
    pub cell_deps: Vec<serde_json::Value>,
    pub header_deps: Vec<String>,
    pub inputs: Vec<serde_json::Value>,
    pub outputs: Vec<CellOutput>,
    pub outputs_data: Vec<String>,
    pub witnesses: Vec<String>,
}
impl Transaction {
    pub fn packed(&self) -> Result<packed::Transaction> {
        let tx: ckb_jsonrpc_types::Transaction =
            serde_json::from_value(serde_json::to_value(self)?)?;
        let tx: ckb_types::packed::Transaction = tx.into();
        packed::Transaction::from_slice(tx.as_slice()).context("Transaction encoding")
    }
    pub fn hash(&self) -> Result<String> {
        Ok(hx(self.packed()?.hash()))
    }
    pub fn input_points(&self) -> Result<Vec<OutPoint>> {
        self.inputs
            .iter()
            .map(|i| serde_json::from_value(i["previous_output"].clone()).map_err(Into::into))
            .collect()
    }
    pub fn cell(&self, point: &OutPoint) -> Result<Cell> {
        ensure!(self.hash()? == point.tx_hash, "Transaction hash mismatch");
        let i = usize::try_from(quantity(&point.index)?)?;
        Ok(Cell {
            out_point: point.clone(),
            output: self.outputs.get(i).context("Missing output")?.clone(),
            output_data: self
                .outputs_data
                .get(i)
                .context("Missing output data")?
                .clone(),
            block_number: String::new(),
            tx_index: String::new(),
        })
    }
}

/// Resolve every executed script to a pinned explicit dependency before exporting.
pub(crate) fn verify_script_dependencies(
    inputs: &[Cell],
    outputs: &[CellOutput],
    deps: &[Cell],
) -> Result<()> {
    for script in inputs
        .iter()
        .map(|c| &c.output.lock)
        .chain(inputs.iter().filter_map(|c| c.output.type_script.as_ref()))
        .chain(outputs.iter().filter_map(|c| c.type_script.as_ref()))
    {
        let matches = deps
            .iter()
            .filter(|c| match script.hash_type.as_str() {
                "type" => {
                    c.output
                        .type_script
                        .as_ref()
                        .and_then(|t| t.hash().ok())
                        .as_ref()
                        == Some(&script.code_hash)
                }
                "data" | "data1" | "data2" => {
                    bytes(&c.output_data).is_ok_and(|b| hx(hash(&b)) == script.code_hash)
                }
                _ => false,
            })
            .count();
        ensure!(
            matches == 1,
            "Executed script must resolve to one pinned code dependency: {}",
            script.code_hash
        );
    }
    Ok(())
}
pub(crate) fn verify_size(tx: &Transaction) -> Result<()> {
    ensure!(
        tx.packed()?.as_slice().len() <= 512_000,
        "Transaction exceeds 512,000-byte size budget; split request"
    );
    Ok(())
}
