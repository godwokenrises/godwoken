use anyhow::Result;
use gw_types::{
    bytes::Bytes,
    packed::{CellDep, CellInput, CellOutput, Transaction, WitnessArgs},
    prelude::Entity,
};

#[derive(Default)]
pub struct TransactionSkeleton {
    inputs: Vec<CellInput>,
    cell_deps: Vec<CellDep>,
    witnesses: Vec<WitnessArgs>,
    cell_outputs: Vec<(CellOutput, Bytes)>,
}

impl TransactionSkeleton {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn inputs(&self) -> &Vec<CellInput> {
        &self.inputs
    }

    pub fn inputs_mut(&mut self) -> &mut Vec<CellInput> {
        &mut self.inputs
    }

    pub fn cell_deps_mut(&mut self) -> &mut Vec<CellDep> {
        &mut self.cell_deps
    }

    pub fn outputs(&self) -> &Vec<(CellOutput, Bytes)> {
        &self.cell_outputs
    }

    pub fn outputs_mut(&mut self) -> &mut Vec<(CellOutput, Bytes)> {
        &mut self.cell_outputs
    }

    pub fn witnesses_mut(&mut self) -> &mut Vec<WitnessArgs> {
        &mut self.witnesses
    }

    pub fn signature_messages(&self) -> Vec<[u8; 32]> {
        unimplemented!()
    }

    pub fn seal(&self, signatures: Vec<[u8; 65]>) -> Result<Transaction> {
        unimplemented!()
    }

    pub fn tx_in_block_size(&self) -> Result<usize> {
        let dummy_signatures = {
            let len = self.signature_messages().len();
            let mut dummy_signatures = Vec::with_capacity(len);
            dummy_signatures.resize(len, [0u8; 65]);
            dummy_signatures
        };
        let tx = self.seal(dummy_signatures)?;
        // tx size + 4 in block serialization cost
        let tx_in_block_size = tx.as_slice().len() + 4;
        Ok(tx_in_block_size)
    }
}
