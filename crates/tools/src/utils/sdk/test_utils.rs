use std::collections::{HashMap, HashSet};
use std::convert::TryFrom;

use anyhow::anyhow;
use ckb_jsonrpc_types::Serialize;
use rand::{thread_rng, Rng};
use thiserror::Error;

use crate::utils::sdk::{
    constants::{
        MULTISIG_GROUP_OUTPUT_LOC, MULTISIG_TYPE_HASH, ONE_CKB, SIGHASH_GROUP_OUTPUT_LOC,
        SIGHASH_TYPE_HASH,
    },
    traits::{
        CellDepResolver, DefaultCellDepResolver, HeaderDepResolver, TransactionDependencyError,
        TransactionDependencyProvider,
    },
    tx_fee::tx_fee,
    ScriptId,
};
use ckb_hash::blake2b_256;
use ckb_mock_tx_types::{
    MockCellDep, MockInfo, MockInput, MockResourceLoader, MockTransaction, Resource,
};
use ckb_script::TransactionScriptsVerifier;
use ckb_types::{
    bytes::Bytes,
    core::{
        cell::resolve_transaction, BlockView, Capacity, Cycle, DepType, FeeRate, HeaderView,
        ScriptHashType, TransactionView,
    },
    packed::{Byte32, CellDep, CellInput, CellOutput, OutPoint, OutPointVec, Script, Transaction},
    prelude::*,
    H256,
};

/// Test utils errors
#[derive(Error, Debug)]
pub enum Error {
    #[error("no enough capacity for holding a cell: occupied={occupied}, capacity={capacity}")]
    NoEnoughCapacityForCell { occupied: u64, capacity: u64 },
    #[error("transaction fee not enough: {0}")]
    NoEnoughFee(String),
    #[error("verify script error: {0}")]
    VerifyScript(String),
    #[error("other error: {0}")]
    Other(String),
}

/// The test context for CKB Rust SDK
#[derive(Clone, Default)]
pub struct Context {
    pub inputs: Vec<MockInput>,
    pub cell_deps: Vec<MockCellDep>,
    pub header_deps: Vec<HeaderView>,

    /// cell dep data hashes
    pub dep_data_hashes: Vec<H256>,
    /// cell dep type script hashes
    pub dep_type_hashes: Vec<Option<H256>>,
    /// For resolve dep group cell dep
    pub cell_dep_map: HashMap<ScriptId, CellDep>,
}

pub struct LiveCellsContext {
    pub inputs: Vec<MockInput>,
    pub header_deps: Vec<HeaderView>,
    pub used_inputs: HashSet<usize>,
}

impl Context {
    /// When the contract is a lock script will combine the deployed out point
    /// with secp256k1_data and map the script id to dep_group cell_dep. The
    /// contracts can only be referenced by data hash and with
    /// hash_type="data1".
    pub fn new(block: &BlockView, contracts: Vec<(&[u8], bool)>) -> Context {
        let block_number: u64 = block.number();
        assert_eq!(block_number, 0);
        let cell_dep_resolver = DefaultCellDepResolver::from_genesis(block).expect("genesis info");
        let block_hash = block.hash();
        let mut ctx = Context::default();
        for (cell_dep, (tx_idx, output_idx)) in [
            (
                cell_dep_resolver.sighash_dep().unwrap().clone().0,
                SIGHASH_GROUP_OUTPUT_LOC,
            ),
            (
                cell_dep_resolver.multisig_dep().unwrap().clone().0,
                MULTISIG_GROUP_OUTPUT_LOC,
            ),
        ] {
            let tx = block.transaction(tx_idx).expect("get tx");
            let (output, data) = tx.output_with_data(output_idx).expect("get output+data");
            ctx.add_cell_dep(cell_dep, output, data, Some(block_hash.clone()));
        }
        for (code_hash, cell_dep) in [
            (
                SIGHASH_TYPE_HASH,
                cell_dep_resolver.sighash_dep().unwrap().0.clone(),
            ),
            (
                MULTISIG_TYPE_HASH,
                cell_dep_resolver.multisig_dep().unwrap().0.clone(),
            ),
        ] {
            ctx.add_cell_dep_map(ScriptId::new_type(code_hash), cell_dep);
        }
        for tx in block.transactions().iter() {
            for (idx, (output, data)) in tx
                .outputs()
                .into_iter()
                .zip(tx.outputs_data().into_iter())
                .enumerate()
            {
                let cell_dep = CellDep::new_builder()
                    .out_point(OutPoint::new(tx.hash(), idx as u32))
                    .dep_type(DepType::Code.into())
                    .build();
                ctx.add_cell_dep(cell_dep, output, data.raw_data(), Some(block_hash.clone()));
            }
        }

        if !contracts.is_empty() {
            let secp_data_out_point = OutPoint::new(block.transaction(0).unwrap().hash(), 3);
            for (bin, is_lock) in contracts {
                let data_hash = H256::from(blake2b_256(bin));
                let out_point = ctx.deploy_cell(Bytes::from(bin.to_vec()));
                if is_lock {
                    let out_points: OutPointVec =
                        vec![secp_data_out_point.clone(), out_point].pack();
                    let group_out_point = ctx.deploy_cell(out_points.as_bytes());
                    let cell_dep = CellDep::new_builder()
                        .out_point(group_out_point)
                        .dep_type(DepType::DepGroup.into())
                        .build();
                    let script_id = ScriptId::new_data1(data_hash);
                    ctx.add_cell_dep_map(script_id, cell_dep);
                }
            }
        }
        ctx.add_header(block.header());
        ctx
    }

    /// Adds a live cell to the set.
    ///
    /// If the set did not have this input present, old live cell is returned.
    ///
    /// If the set did have this input present, None is returned.
    pub fn add_live_cell(
        &mut self,
        input: CellInput,
        output: CellOutput,
        data: Bytes,
        header: Option<Byte32>,
    ) -> Option<(CellOutput, Bytes, Option<Byte32>)> {
        for mock_input in &mut self.inputs {
            if mock_input.input == input {
                let old_output = mock_input.output.clone();
                let old_data = mock_input.data.clone();
                let old_header = mock_input.header.clone();
                mock_input.output = output;
                mock_input.data = data;
                mock_input.header = header;
                return Some((old_output, old_data, old_header));
            }
        }
        self.inputs.push(MockInput {
            input,
            output,
            data,
            header,
        });
        None
    }

    /// Add a live cell with empty data
    pub fn add_simple_live_cell(
        &mut self,
        out_point: OutPoint,
        lock_script: Script,
        capacity: Option<u64>,
    ) -> Option<(CellOutput, Bytes, Option<Byte32>)> {
        let input = CellInput::new(out_point, 0);
        let capacity = capacity.unwrap_or_else(|| {
            let lock_size = 33 + lock_script.args().raw_data().len();
            Capacity::bytes((8 + lock_size) * ONE_CKB as usize)
                .unwrap()
                .as_u64()
        });
        let output = CellOutput::new_builder()
            .capacity(capacity.pack())
            .lock(lock_script)
            .build();
        self.add_live_cell(input, output, Bytes::default(), None)
    }

    /// Deploy a cell
    /// return the out-point of the cell
    pub fn deploy_cell(&mut self, data: Bytes) -> OutPoint {
        let out_point = random_out_point();
        let cell_dep = CellDep::new_builder()
            .out_point(out_point.clone())
            .dep_type(DepType::Code.into())
            .build();
        let output = CellOutput::default();
        self.add_cell_dep(cell_dep, output, data, None);
        out_point
    }

    pub fn add_cell_dep(
        &mut self,
        cell_dep: CellDep,
        output: CellOutput,
        data: Bytes,
        header: Option<Byte32>,
    ) -> Option<(CellOutput, Bytes, Option<Byte32>)> {
        let data_hash = H256::from(blake2b_256(data.as_ref()));
        let script_hash_opt = output
            .type_()
            .to_opt()
            .map(|script| H256::from(blake2b_256(script.as_slice())));
        for (idx, mock_cell_dep) in self.cell_deps.iter_mut().enumerate() {
            if mock_cell_dep.cell_dep == cell_dep {
                let old_output = mock_cell_dep.output.clone();
                let old_data = mock_cell_dep.data.clone();
                let old_header = mock_cell_dep.header.clone();
                mock_cell_dep.output = output;
                mock_cell_dep.data = data;
                mock_cell_dep.header = header;
                self.dep_data_hashes[idx] = data_hash;
                self.dep_type_hashes[idx] = script_hash_opt;
                return Some((old_output, old_data, old_header));
            }
        }
        self.cell_deps.push(MockCellDep {
            cell_dep,
            output,
            data,
            header,
        });
        self.dep_data_hashes.push(data_hash);
        self.dep_type_hashes.push(script_hash_opt);
        None
    }

    pub fn add_cell_dep_map(&mut self, script_id: ScriptId, cell_dep: CellDep) -> Option<CellDep> {
        self.cell_dep_map.insert(script_id, cell_dep)
    }

    pub fn add_header(&mut self, header: HeaderView) {
        self.header_deps.push(header);
    }

    pub fn get_live_cell(&self, out_point: &OutPoint) -> Option<(CellOutput, Bytes)> {
        if let Some(result) = self.get_input(out_point) {
            return Some(result);
        }
        for mock_cell_dep in &self.cell_deps {
            if out_point == &mock_cell_dep.cell_dep.out_point() {
                return Some((mock_cell_dep.output.clone(), mock_cell_dep.data.clone()));
            }
        }
        None
    }
    pub fn get_input(&self, out_point: &OutPoint) -> Option<(CellOutput, Bytes)> {
        for mock_input in &self.inputs {
            if out_point == &mock_input.input.previous_output() {
                return Some((mock_input.output.clone(), mock_input.data.clone()));
            }
        }
        None
    }

    pub fn to_mock_tx(&self, tx: Transaction) -> MockTransaction {
        let mock_info = MockInfo {
            inputs: self.inputs.clone(),
            cell_deps: self.cell_deps.clone(),
            header_deps: self.header_deps.clone(),
        };
        MockTransaction { mock_info, tx }
    }

    pub fn to_live_cells_context(&self) -> LiveCellsContext {
        LiveCellsContext {
            inputs: self.inputs.clone(),
            header_deps: self.header_deps.clone(),
            used_inputs: Default::default(),
        }
    }

    /// Check if the transaction fee is greater than fee rate
    pub fn verify_tx_fee(&self, tx: &TransactionView, fee_rate: u64) -> Result<(), Error> {
        let min_fee = FeeRate::from_u64(fee_rate)
            .fee(tx.data().as_reader().serialized_size_in_block())
            .as_u64();
        let fee = tx_fee(tx.clone(), self, self).map_err(|err| Error::Other(err.to_string()))?;
        if fee < min_fee {
            return Err(Error::NoEnoughFee(format!(
                "min-fee: {}, actual-fee: {}",
                min_fee, fee
            )));
        }
        Ok(())
    }

    /// Run all scripts in the transaction in ckb-vm
    pub fn verify_scripts(&self, tx: TransactionView) -> Result<Cycle, Error> {
        let mock_tx = self.to_mock_tx(tx.data());
        let resource = Resource::from_both(&mock_tx, DummyLoader).map_err(Error::VerifyScript)?;
        let rtx = resolve_transaction(tx, &mut HashSet::new(), &resource, &resource)
            .map_err(|err| Error::VerifyScript(format!("Resolve transaction error: {:?}", err)))?;

        let mut verifier = TransactionScriptsVerifier::new(&rtx, &resource);
        verifier.set_debug_printer(|script_hash, message| {
            println!("script: {:x}, debug: {}", script_hash, message);
        });
        verifier
            .verify(u64::max_value())
            .map_err(|err| Error::VerifyScript(format!("Verify script error: {:?}", err)))
    }

    /// Verify:
    ///  * the transaction fee is greater than fee rate
    ///  * run the transaction in ckb-vm
    pub fn verify(&self, tx: TransactionView, fee_rate: u64) -> Result<Cycle, Error> {
        self.verify_tx_fee(&tx, fee_rate)?;
        self.verify_scripts(tx)
    }
}

impl TransactionDependencyProvider for Context {
    // For verify certain cell belong to certain transaction
    fn get_transaction(
        &self,
        _tx_hash: &Byte32,
    ) -> Result<TransactionView, TransactionDependencyError> {
        Err(TransactionDependencyError::Other(anyhow!(
            "context get_transaction"
        )))
    }
    // For get the output information of inputs or cell_deps, those cell should be live cell
    fn get_cell(&self, out_point: &OutPoint) -> Result<CellOutput, TransactionDependencyError> {
        self.get_live_cell(out_point)
            .map(|(output, _)| output)
            .ok_or_else(|| TransactionDependencyError::NotFound("cell not found".to_string()))
    }
    // For get the output data information of inputs or cell_deps
    fn get_cell_data(&self, out_point: &OutPoint) -> Result<Bytes, TransactionDependencyError> {
        self.get_live_cell(out_point)
            .map(|(_, data)| data)
            .ok_or_else(|| TransactionDependencyError::NotFound("cell data not found".to_string()))
    }
    // For get the header information of header_deps
    fn get_header(&self, _block_hash: &Byte32) -> Result<HeaderView, TransactionDependencyError> {
        Err(TransactionDependencyError::NotFound(
            "header not found".to_string(),
        ))
    }
}

impl HeaderDepResolver for Context {
    fn resolve_by_tx(&self, tx_hash: &Byte32) -> Result<Option<HeaderView>, anyhow::Error> {
        let mut header_opt = None;
        for item in &self.inputs {
            if item.input.previous_output().tx_hash() == *tx_hash {
                header_opt = item.header.clone();
            }
        }
        if header_opt.is_none() {
            for item in &self.cell_deps {
                if item.cell_dep.out_point().tx_hash() == *tx_hash {
                    header_opt = item.header.clone();
                }
            }
        }
        if let Some(hash) = header_opt {
            for mock_header in &self.header_deps {
                if hash == mock_header.hash() {
                    return Ok(Some(mock_header.clone()));
                }
            }
        }
        Ok(None)
    }
    fn resolve_by_number(&self, number: u64) -> Result<Option<HeaderView>, anyhow::Error> {
        for mock_header in &self.header_deps {
            if number == mock_header.number() {
                return Ok(Some(mock_header.clone()));
            }
        }
        Ok(None)
    }
}

impl CellDepResolver for Context {
    fn resolve(&self, script: &Script) -> Option<CellDep> {
        let code_hash: H256 = script.code_hash().unpack();
        let hash_type = script.hash_type();
        let script_id = ScriptId::new(
            code_hash.clone(),
            ScriptHashType::try_from(hash_type).unwrap(),
        );
        if let Some(cell_dep) = self.cell_dep_map.get(&script_id) {
            return Some(cell_dep.clone());
        }
        if hash_type == ScriptHashType::Type.into() {
            for (idx, hash_opt) in self.dep_type_hashes.iter().enumerate() {
                if hash_opt.as_ref() == Some(&code_hash) {
                    return Some(self.cell_deps[idx].cell_dep.clone());
                }
            }
        } else {
            for (idx, hash) in self.dep_data_hashes.iter().enumerate() {
                if *hash == code_hash {
                    return Some(self.cell_deps[idx].cell_dep.clone());
                }
            }
        }
        None
    }
}

struct DummyLoader;
impl MockResourceLoader for DummyLoader {
    fn get_header(&mut self, hash: H256) -> Result<Option<HeaderView>, String> {
        Err(format!("Can not call header getter, hash={:?}", hash))
    }
    fn get_live_cell(
        &mut self,
        out_point: OutPoint,
    ) -> Result<Option<(CellOutput, Bytes, Option<Byte32>)>, String> {
        Err(format!(
            "Can not call live cell getter, out_point={:?}",
            out_point
        ))
    }
}

pub fn random_out_point() -> OutPoint {
    let mut rng = thread_rng();
    let tx_hash = {
        let mut buf = [0u8; 32];
        rng.fill(&mut buf);
        buf.pack()
    };
    OutPoint::new(tx_hash, 0)
}

#[derive(serde::Serialize)]
pub struct MockRpcResult<T> {
    id: u64,
    jsonrpc: String,
    result: T,
}

impl<T: Serialize> MockRpcResult<T> {
    pub fn new(result: T) -> Self {
        Self {
            id: 42,
            jsonrpc: "2.0".to_string(),
            result,
        }
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(&self).unwrap()
    }
}

#[cfg(test)]
mod anyhow_tests {
    use anyhow::anyhow;
    #[test]
    fn test_error() {
        let error = super::Error::VerifyScript("VerifyScript".to_string());
        let error = anyhow!(error);
        assert_eq!("verify script error: VerifyScript", error.to_string())
    }
}
