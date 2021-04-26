use crate::types::InputCellInfo;
use crate::{rpc_client::RPCClient, transaction_skeleton::TransactionSkeleton};
use anyhow::{anyhow, Result};
use async_jsonrpc_client::Output;
use gw_common::{blake2b::new_blake2b, H256};
use gw_types::{
    core::DepType,
    packed::{Block, CellDep, CellInput, CellOutput, Header, OutPoint, Script},
    prelude::*,
};
use serde::de::DeserializeOwned;
use serde_json::from_value;

// convert json output to result
pub fn to_result<T: DeserializeOwned>(output: Output) -> Result<T> {
    match output {
        Output::Success(success) => Ok(from_value(success.result)?),
        Output::Failure(failure) => Err(anyhow!("JSONRPC error: {}", failure.error)),
    }
}

/// Calculate tx fee
/// TODO accept fee rate args
fn calculate_required_tx_fee(tx_size: usize) -> u64 {
    // tx_size * KB / MIN_FEE_RATE
    tx_size as u64
}

/// Add fee cell to tx skeleton
pub async fn fill_tx_fee(
    tx_skeleton: &mut TransactionSkeleton,
    rpc_client: &RPCClient,
    lock_script: Script,
) -> Result<()> {
    const CHANGE_CELL_CAPACITY: u64 = 61_00000000;

    let tx_size = tx_skeleton.tx_in_block_size()?;
    let paid_fee: u64 = tx_skeleton.calculate_fee()?;
    let taken_outpoints = tx_skeleton.taken_outpoints()?;
    // calculate required fee
    let required_fee = calculate_required_tx_fee(tx_size).saturating_sub(paid_fee);

    // get payment cells
    // we assume always need a change cell to simplify the code
    let cells = rpc_client
        .query_payment_cells(
            lock_script.clone(),
            required_fee + CHANGE_CELL_CAPACITY,
            &taken_outpoints,
        )
        .await?;
    assert!(!cells.is_empty(), "need cells to pay fee");
    // put cells in tx skeleton
    tx_skeleton
        .inputs_mut()
        .extend(cells.into_iter().map(|cell| {
            let input = CellInput::new_builder()
                .previous_output(cell.out_point.clone())
                .build();
            InputCellInfo { input, cell }
        }));

    // Generate change cell
    let change_capacity = {
        let paid_fee: u64 = tx_skeleton.calculate_fee()?;
        // calculate required fee
        let required_fee = calculate_required_tx_fee(tx_size).saturating_sub(paid_fee);
        paid_fee - required_fee
    };

    assert!(
        change_capacity > CHANGE_CELL_CAPACITY,
        "change capacity must cover the change cell"
    );

    let change_cell = CellOutput::new_builder()
        .lock(lock_script)
        .capacity(change_capacity.pack())
        .build();

    tx_skeleton
        .outputs_mut()
        .push((change_cell, Default::default()));
    Ok(())
}

#[derive(Debug, Clone)]
pub struct CKBGenesisInfo {
    header: Header,
    out_points: Vec<Vec<OutPoint>>,
    sighash_data_hash: H256,
    sighash_type_hash: H256,
    multisig_data_hash: H256,
    multisig_type_hash: H256,
    dao_data_hash: H256,
    dao_type_hash: H256,
}

impl CKBGenesisInfo {
    // Special cells in genesis transactions: (transaction-index, output-index)
    pub const SIGHASH_OUTPUT_LOC: (usize, usize) = (0, 1);
    pub const MULTISIG_OUTPUT_LOC: (usize, usize) = (0, 4);
    pub const DAO_OUTPUT_LOC: (usize, usize) = (0, 2);
    pub const SIGHASH_GROUP_OUTPUT_LOC: (usize, usize) = (1, 0);
    pub const MULTISIG_GROUP_OUTPUT_LOC: (usize, usize) = (1, 1);

    pub fn from_block(genesis_block: &Block) -> Result<Self> {
        let raw_header = genesis_block.header().raw();
        let number: u64 = raw_header.number().unpack();
        if number != 0 {
            return Err(anyhow!("Invalid genesis block number: {}", number));
        }

        let mut sighash_data_hash = None;
        let mut sighash_type_hash = None;
        let mut multisig_data_hash = None;
        let mut multisig_type_hash = None;
        let mut dao_data_hash = None;
        let mut dao_type_hash = None;
        let out_points = genesis_block
            .transactions()
            .into_iter()
            .enumerate()
            .map(|(tx_index, tx)| {
                let raw_tx = tx.raw();
                raw_tx
                    .outputs()
                    .into_iter()
                    .zip(raw_tx.outputs_data().into_iter())
                    .enumerate()
                    .map(|(index, (output, data))| {
                        let data_hash: H256 = {
                            let mut hasher = new_blake2b();
                            hasher.update(&data.raw_data());
                            let mut hash = [0u8; 32];
                            hasher.finalize(&mut hash);
                            hash.into()
                        };
                        if tx_index == Self::SIGHASH_OUTPUT_LOC.0
                            && index == Self::SIGHASH_OUTPUT_LOC.1
                        {
                            sighash_type_hash =
                                output.type_().to_opt().map(|script| script.hash().into());
                            sighash_data_hash = Some(data_hash);
                        }
                        if tx_index == Self::MULTISIG_OUTPUT_LOC.0
                            && index == Self::MULTISIG_OUTPUT_LOC.1
                        {
                            multisig_type_hash =
                                output.type_().to_opt().map(|script| script.hash().into());
                            multisig_data_hash = Some(data_hash);
                        }
                        if tx_index == Self::DAO_OUTPUT_LOC.0 && index == Self::DAO_OUTPUT_LOC.1 {
                            dao_type_hash =
                                output.type_().to_opt().map(|script| script.hash().into());
                            dao_data_hash = Some(data_hash);
                        }
                        let tx_hash = {
                            let mut hasher = new_blake2b();
                            hasher.update(tx.raw().as_slice());
                            let mut hash = [0u8; 32];
                            hasher.finalize(&mut hash);
                            hash
                        };
                        OutPoint::new_builder()
                            .tx_hash(tx_hash.pack())
                            .index((index as u32).pack())
                            .build()
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();

        let sighash_data_hash =
            sighash_data_hash.ok_or_else(|| anyhow!("No data hash(sighash) found in txs[0][1]"))?;
        let sighash_type_hash =
            sighash_type_hash.ok_or_else(|| anyhow!("No type hash(sighash) found in txs[0][1]"))?;
        let multisig_data_hash = multisig_data_hash
            .ok_or_else(|| anyhow!("No data hash(multisig) found in txs[0][4]"))?;
        let multisig_type_hash = multisig_type_hash
            .ok_or_else(|| anyhow!("No type hash(multisig) found in txs[0][4]"))?;
        let dao_data_hash =
            dao_data_hash.ok_or_else(|| anyhow!("No data hash(dao) found in txs[0][2]"))?;
        let dao_type_hash =
            dao_type_hash.ok_or_else(|| anyhow!("No type hash(dao) found in txs[0][2]"))?;
        Ok(CKBGenesisInfo {
            header: genesis_block.header(),
            out_points,
            sighash_data_hash,
            sighash_type_hash,
            multisig_data_hash,
            multisig_type_hash,
            dao_data_hash,
            dao_type_hash,
        })
    }

    pub fn header(&self) -> &Header {
        &self.header
    }

    pub fn sighash_data_hash(&self) -> &H256 {
        &self.sighash_data_hash
    }

    pub fn sighash_type_hash(&self) -> &H256 {
        &self.sighash_type_hash
    }

    pub fn multisig_data_hash(&self) -> &H256 {
        &self.multisig_data_hash
    }

    pub fn multisig_type_hash(&self) -> &H256 {
        &self.multisig_type_hash
    }

    pub fn dao_data_hash(&self) -> &H256 {
        &self.dao_data_hash
    }

    pub fn dao_type_hash(&self) -> &H256 {
        &self.dao_type_hash
    }

    pub fn sighash_dep(&self) -> CellDep {
        CellDep::new_builder()
            .out_point(
                self.out_points[Self::SIGHASH_GROUP_OUTPUT_LOC.0][Self::SIGHASH_GROUP_OUTPUT_LOC.1]
                    .clone(),
            )
            .dep_type(DepType::DepGroup.into())
            .build()
    }

    pub fn multisig_dep(&self) -> CellDep {
        CellDep::new_builder()
            .out_point(
                self.out_points[Self::MULTISIG_GROUP_OUTPUT_LOC.0]
                    [Self::MULTISIG_GROUP_OUTPUT_LOC.1]
                    .clone(),
            )
            .dep_type(DepType::DepGroup.into())
            .build()
    }

    pub fn dao_dep(&self) -> CellDep {
        CellDep::new_builder()
            .out_point(self.out_points[Self::DAO_OUTPUT_LOC.0][Self::DAO_OUTPUT_LOC.1].clone())
            .build()
    }
}
