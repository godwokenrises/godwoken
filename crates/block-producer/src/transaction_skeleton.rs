use crate::types::{InputCellInfo, SignatureEntry};
use anyhow::{anyhow, Result};
use gw_types::{
    bytes::Bytes,
    packed::{CellDep, CellOutput, RawTransaction, Transaction, WitnessArgs},
    prelude::*,
};
use std::collections::HashMap;

pub struct SealedTransaction {
    pub transaction: Transaction,
    pub fee: u64,
}

impl SealedTransaction {
    pub fn check_fee_rate(&self) -> Result<()> {
        let tx_in_block_size = self.transaction.as_slice().len() + 4;
        // tx_in_block_size * 1000(min fee rate per KB) / 1000(KB)
        let expected_fee = tx_in_block_size as u64;

        if self.fee < expected_fee {
            return Err(anyhow!(
                "Insufficient tx fee, expected_fee: {}, tx_fee: {}",
                expected_fee,
                self.fee
            ));
        }
        Ok(())
    }
}

#[derive(Default)]
pub struct TransactionSkeleton {
    inputs: Vec<InputCellInfo>,
    cell_deps: Vec<CellDep>,
    witnesses: Vec<WitnessArgs>,
    cell_outputs: Vec<(CellOutput, Bytes)>,
}

impl TransactionSkeleton {
    pub fn inputs(&self) -> &Vec<InputCellInfo> {
        &self.inputs
    }

    pub fn inputs_mut(&mut self) -> &mut Vec<InputCellInfo> {
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

    pub fn signature_entries(&self) -> Vec<SignatureEntry> {
        let mut entries: HashMap<[u8; 32], SignatureEntry> = Default::default();
        for (index, input) in self.inputs.iter().enumerate() {
            let lock_hash = input.cell.output.lock().hash();
            let entry = entries.entry(lock_hash).or_insert_with(|| SignatureEntry {
                lock_hash,
                indexes: Vec::new(),
            });
            entry.indexes.push(index);
        }

        entries.values().cloned().collect()
    }

    pub fn seal(
        &self,
        entries: &[SignatureEntry],
        signatures: Vec<[u8; 65]>,
    ) -> Result<SealedTransaction> {
        assert_eq!(entries.len(), signatures.len());
        // build raw tx
        let inputs = self
            .inputs
            .iter()
            .map(|input_cell| &input_cell.input)
            .cloned();
        let outputs = self
            .outputs()
            .iter()
            .map(|(output, _data)| output.to_owned())
            .collect::<Vec<_>>();
        let outputs_data = self
            .outputs()
            .iter()
            .map(|(_output, data)| data.to_owned())
            .collect::<Vec<_>>();
        let raw_tx = RawTransaction::new_builder()
            .inputs(inputs.pack())
            .outputs(outputs.pack())
            .outputs_data(outputs_data.pack())
            .cell_deps(self.cell_deps.clone().pack())
            .build();

        // build witnesses
        let mut witnesses: Vec<WitnessArgs> = self.witnesses.clone();
        if witnesses.len() < self.inputs.len() {
            witnesses.resize(self.inputs.len(), Default::default());
        }
        // set signature to witnesses
        for (entry, signature) in entries.iter().zip(signatures) {
            let witness_args = witnesses
                .get_mut(entry.indexes[0])
                .expect("can't find witness");
            if witness_args.lock().is_some() {
                return Err(anyhow!(
                    "entry signature conflict with the witness index: {}",
                    entry.indexes[0]
                ));
            }
            *witness_args = witness_args
                .to_owned()
                .as_builder()
                .lock(Some(Bytes::from(signature.to_vec())).pack())
                .build();
        }

        let witnesses = witnesses
            .into_iter()
            .map(|args| args.as_bytes())
            .collect::<Vec<_>>();
        let transaction = Transaction::new_builder()
            .raw(raw_tx)
            .witnesses(witnesses.pack())
            .build();
        let fee = self.calculate_fee()?;

        let sealed = SealedTransaction { transaction, fee };
        Ok(sealed)
    }

    pub fn calculate_fee(&self) -> Result<u64> {
        let inputs_capacity: u64 = self
            .inputs
            .iter()
            .map(|input| {
                let capacity: u64 = input.cell.output.capacity().unpack();
                capacity
            })
            .sum();

        let outputs_capacity: u64 = self
            .cell_outputs
            .iter()
            .map(|(output, _data)| {
                let capacity: u64 = output.capacity().unpack();
                capacity
            })
            .sum();

        let tx_fee = inputs_capacity.saturating_sub(outputs_capacity);
        Ok(tx_fee)
    }

    pub fn tx_in_block_size(&self) -> Result<usize> {
        let entries = self.signature_entries();
        let dummy_signatures = {
            let mut dummy_signatures = Vec::with_capacity(entries.len());
            dummy_signatures.resize(entries.len(), [0u8; 65]);
            dummy_signatures
        };
        let sealed_tx = self.seal(&entries, dummy_signatures)?;
        // tx size + 4 in block serialization cost
        let tx_in_block_size = sealed_tx.transaction.as_slice().len() + 4;
        Ok(tx_in_block_size)
    }
}
