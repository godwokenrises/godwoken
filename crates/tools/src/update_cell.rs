use anyhow::{anyhow, Result};
use ckb_jsonrpc_types::OutputsValidator;
use ckb_sdk::HttpRpcClient;
use ckb_types::prelude::{Entity, Unpack as CKBUnpack};
use gw_config::WalletConfig;
use gw_rpc_client::indexer_client::CKBIndexerClient;
use gw_types::{
    offchain::{CellInfo, InputCellInfo},
    packed::{CellInput, CellOutput, OutPoint},
    prelude::*,
};
use gw_utils::{
    fee::fill_tx_fee, genesis_info::CKBGenesisInfo, transaction_skeleton::TransactionSkeleton,
    wallet::Wallet,
};
use std::path::{Path, PathBuf};

use crate::utils::transaction::wait_for_tx;

pub async fn update_cell<P: AsRef<Path>>(
    ckb_rpc_url: &str,
    indexer_rpc_url: &str,
    tx_hash: [u8; 32],
    index: u32,
    type_id: [u8; 32],
    cell_data_path: P,
    pk_path: PathBuf,
) -> Result<()> {
    let mut rpc_client = HttpRpcClient::new(ckb_rpc_url.to_string());
    let indexer_client = CKBIndexerClient::with_url(indexer_rpc_url)?;
    // check existed_cell
    let tx_with_status = rpc_client
        .get_transaction(tx_hash.into())
        .map_err(|err| anyhow!("{}", err))?
        .ok_or_else(|| anyhow!("can't found transaction"))?;
    let tx = tx_with_status.transaction.inner;
    let existed_cell = tx
        .outputs
        .get(index as usize)
        .ok_or_else(|| anyhow!("can't found cell"))?;
    let existed_cell_data = tx
        .outputs_data
        .get(index as usize)
        .ok_or_else(|| anyhow!("can't found cell data"))?;
    // check type_id
    let type_: ckb_types::packed::Script = existed_cell
        .clone()
        .type_
        .ok_or_else(|| anyhow!("can't found type_id from existed cell"))?
        .into();
    let existed_cell_type_id: [u8; 32] = type_.calc_script_hash().unpack();
    assert_eq!(
        hex::encode(existed_cell_type_id),
        hex::encode(type_id),
        "check existed cell type id"
    );
    // read new cell data
    let new_cell_data = std::fs::read(&cell_data_path)?;
    // generate new cell
    let existed_cell = {
        let existed_cell: ckb_types::packed::CellOutput = existed_cell.to_owned().into();
        CellOutput::new_unchecked(existed_cell.as_bytes())
    };
    let new_cell_capacity = existed_cell.occupied_capacity(new_cell_data.len())?;
    let new_cell = existed_cell
        .clone()
        .as_builder()
        .capacity(new_cell_capacity.pack())
        .build();
    // get genesis info
    let ckb_genesis_info = {
        let ckb_genesis = rpc_client
            .get_block_by_number(0u64)
            .map_err(|err| anyhow!("{}", err))?
            .ok_or_else(|| anyhow!("can't found CKB genesis block"))?;
        let block: ckb_types::core::BlockView = ckb_genesis.into();
        let block = gw_types::packed::Block::new_unchecked(block.data().as_bytes());
        CKBGenesisInfo::from_block(&block)?
    };
    // build tx
    let mut tx_skeleton = TransactionSkeleton::default();
    let out_point = OutPoint::new_builder()
        .tx_hash(tx_hash.pack())
        .index(index.pack())
        .build();
    let input = InputCellInfo {
        input: CellInput::new_builder()
            .previous_output(out_point.clone())
            .build(),
        cell: CellInfo {
            out_point,
            output: existed_cell.clone(),
            data: existed_cell_data.clone().into_bytes(),
        },
    };
    tx_skeleton.inputs_mut().push(input);
    tx_skeleton
        .outputs_mut()
        .push((new_cell, new_cell_data.into()));
    // secp256k1 lock, used for unlock tx fee payment cells
    tx_skeleton
        .cell_deps_mut()
        .push(ckb_genesis_info.sighash_dep());
    // use same lock of existed cell to pay fee
    let payment_lock = existed_cell.lock();
    // tx fee cell
    fill_tx_fee(&mut tx_skeleton, &indexer_client, payment_lock.clone()).await?;
    // sign
    let wallet = Wallet::from_config(&WalletConfig {
        privkey_path: pk_path,
        lock: payment_lock.into(),
    })?;
    let tx = wallet.sign_tx_skeleton(tx_skeleton)?;
    let update_message = format!(
        "tx hash: {} cell index: 0 size: {}",
        hex::encode(tx.hash()),
        tx.as_slice().len()
    );
    println!("{}", update_message);
    // send transaction
    println!("Unlock cell {}", existed_cell.lock());
    let tx_hash = rpc_client
        .send_transaction(
            ckb_types::packed::Transaction::new_unchecked(tx.as_bytes()),
            Some(OutputsValidator::Passthrough),
        )
        .map_err(|err| anyhow!("{}", err))?;
    println!("Send tx...");
    wait_for_tx(&mut rpc_client, &tx_hash, 180).map_err(|err| anyhow!("{}", err))?;
    println!("{}", update_message);
    println!("Cell is updated!");
    Ok(())
}
