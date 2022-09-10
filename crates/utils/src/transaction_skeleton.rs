#![allow(clippy::mutable_key_type)]

use anyhow::{anyhow, Result};
use gw_types::{
    bytes::Bytes,
    offchain::{CellInfo, InputCellInfo},
    packed::{
        CellDep, CellInput, CellOutput, OmniLockWitnessLock, OutPoint, RawTransaction, Transaction,
        WitnessArgs,
    },
    prelude::*,
};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Copy)]
pub enum SignatureKind {
    OmniLockSecp256k1,
    GenesisSecp256k1,
}

#[derive(Clone)]
pub struct SignatureEntry {
    pub indexes: Vec<usize>,
    pub lock_hash: [u8; 32],
    pub kind: SignatureKind,
}

#[derive(Debug)]
pub enum Signature {
    OmniLockSecp256k1(OmniLockWitnessLock),
    GenesisSecp256k1([u8; 65]),
}

impl Signature {
    pub fn new(kind: SignatureKind, sig: [u8; 65]) -> Self {
        match kind {
            SignatureKind::OmniLockSecp256k1 => Signature::OmniLockSecp256k1(
                OmniLockWitnessLock::new_builder()
                    .signature(Some(Bytes::from(sig.to_vec())).pack())
                    .build(),
            ),
            SignatureKind::GenesisSecp256k1 => Signature::GenesisSecp256k1(sig),
        }
    }

    pub fn zero_bytes_from_entry(entry: &SignatureEntry) -> Bytes {
        let len = Self::new(entry.kind, [0u8; 65]).as_bytes().len();
        let mut buf = Vec::new();
        buf.resize(len, 0);
        Bytes::from(buf)
    }

    pub fn as_bytes(&self) -> Bytes {
        match self {
            Signature::OmniLockSecp256k1(sig) => sig.as_bytes(),
            Signature::GenesisSecp256k1(sig) => Bytes::from(sig.to_vec()),
        }
    }
}

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
    omni_lock_code_hash: Option<[u8; 32]>,
    // TODO: refactor
    // Used for `fill_tx_fee` to exclude some outpoints
    excluded_out_points: HashSet<OutPoint>,
    live_cells: Vec<CellInfo>,
}

impl TransactionSkeleton {
    pub fn new(omni_lock_code_hash: [u8; 32]) -> Self {
        TransactionSkeleton {
            omni_lock_code_hash: Some(omni_lock_code_hash),
            ..Default::default()
        }
    }

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

    pub fn witnesses(&self) -> &Vec<WitnessArgs> {
        &self.witnesses
    }

    pub fn witnesses_mut(&mut self) -> &mut Vec<WitnessArgs> {
        &mut self.witnesses
    }

    pub fn omni_lock_code_hash(&self) -> Option<&[u8; 32]> {
        self.omni_lock_code_hash.as_ref()
    }

    pub fn excluded_out_points_mut(&mut self) -> &mut HashSet<OutPoint> {
        &mut self.excluded_out_points
    }

    pub fn live_cells_mut(&mut self) -> &mut Vec<CellInfo> {
        &mut self.live_cells
    }

    pub fn add_owner_cell(&mut self, owner_cell: CellInfo) {
        self.inputs_mut().push({
            InputCellInfo {
                input: CellInput::new_builder()
                    .previous_output(owner_cell.out_point.clone())
                    .build(),
                cell: owner_cell.clone(),
            }
        });
        self.outputs_mut()
            .push((owner_cell.output, owner_cell.data));
    }

    pub fn signature_entries(&self) -> Vec<SignatureEntry> {
        let mut entries: HashMap<[u8; 32], SignatureEntry> = Default::default();
        for (index, input) in self.inputs.iter().enumerate() {
            // Skip withdrawal lock witness args
            if let Some(witness_args) = self.witnesses().get(index) {
                if witness_args.lock().to_opt().is_some() {
                    continue;
                }
            }

            let lock = input.cell.output.lock();
            let lock_hash = lock.hash();
            let kind = if Some(lock.code_hash().unpack()) == self.omni_lock_code_hash {
                SignatureKind::OmniLockSecp256k1
            } else {
                SignatureKind::GenesisSecp256k1
            };
            let entry = entries.entry(lock_hash).or_insert_with(|| SignatureEntry {
                lock_hash,
                indexes: Vec::new(),
                kind,
            });
            entry.indexes.push(index);
        }

        entries.values().cloned().collect()
    }

    pub fn seal(
        &self,
        entries: &[SignatureEntry],
        signatures: Vec<Bytes>,
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
                .lock(Some(signature).pack())
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
        let dummy_signatures: Vec<_> = {
            let entries = entries.iter();
            entries.map(Signature::zero_bytes_from_entry).collect()
        };
        let sealed_tx = self.seal(&entries, dummy_signatures)?;
        // tx size + 4 in block serialization cost
        let tx_in_block_size = sealed_tx.transaction.as_slice().len() + 4;
        Ok(tx_in_block_size)
    }

    pub fn taken_outpoints(&self) -> Result<HashSet<OutPoint>> {
        let mut taken_outpoints = self.excluded_out_points.clone();
        for (index, input) in self.inputs().iter().enumerate() {
            if !taken_outpoints.insert(input.cell.out_point.clone()) {
                panic!("Duplicated input: {:?}, index: {}", input, index);
            }
        }
        Ok(taken_outpoints)
    }

    pub fn live_cells(&self) -> &Vec<CellInfo> {
        &self.live_cells
    }
}
