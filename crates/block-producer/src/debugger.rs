use std::{
    fs::{create_dir_all, write},
    path::{Path, PathBuf},
};

use anyhow::{anyhow, Result};
use ckb_types::prelude::Entity;
use gw_jsonrpc_types::{
    ckb_jsonrpc_types,
    debugger::{ReprMockCellDep, ReprMockInfo, ReprMockInput, ReprMockTransaction},
};
use gw_rpc_client::rpc_client::RPCClient;
use gw_types::h256::*;
use gw_types::{
    core::DepType,
    packed::{CellDep, OutPointVec, Transaction},
    prelude::*,
};

pub async fn dump_transaction<P: AsRef<Path>>(
    dir: P,
    rpc_client: &RPCClient,
    tx: &Transaction,
) -> Result<()> {
    // ensure dir is exist
    create_dir_all(&dir)?;

    let tx_hash: ckb_types::H256 = tx.hash().into();
    log::info!("Build mock transaction {}", tx_hash);

    let mut dump_path = PathBuf::new();
    dump_path.push(dir);
    let json_content = match build_mock_transaction(rpc_client, tx).await {
        Ok(mock_tx) => {
            dump_path.push(format!("{}-mock-tx.json", tx_hash));
            serde_json::to_string_pretty(&mock_tx)?
        }
        Err(err) => {
            log::error!(
                "Failed to build mock transaction {}, error: {}",
                tx_hash,
                err
            );
            log::error!("Fallback to raw tx...");
            dump_path.push(format!("{}-raw-tx.json", tx_hash));
            let json_tx: ckb_jsonrpc_types::Transaction =
                { ckb_types::packed::Transaction::new_unchecked(tx.as_bytes()).into() };
            serde_json::to_string_pretty(&json_tx)?
        }
    };
    log::info!("Dump transaction {} to {:?}", tx_hash, dump_path);
    write(dump_path, json_content)?;
    Ok(())
}

pub async fn build_mock_transaction(
    rpc_client: &RPCClient,
    tx: &Transaction,
) -> Result<ReprMockTransaction> {
    async fn resolve_dep_group(rpc_client: &RPCClient, dep: &CellDep) -> Result<Vec<CellDep>> {
        // return dep
        if dep.dep_type() == DepType::Code.into() {
            return Ok(vec![]);
        }
        // parse dep group
        let cell = match rpc_client
            .get_cell(dep.out_point())
            .await?
            .and_then(|cell_status| cell_status.cell)
        {
            Some(cell) => Some(cell),
            None => rpc_client.get_cell_from_mempool(dep.out_point()).await?,
        }
        .ok_or_else(|| anyhow!("can't find dep group cell"))?;
        let out_points =
            OutPointVec::from_slice(&cell.data).map_err(|_| anyhow!("invalid dep group"))?;
        let cell_deps = out_points
            .into_iter()
            .map(|out_point| {
                CellDep::new_builder()
                    .out_point(out_point)
                    .dep_type(DepType::Code.into())
                    .build()
            })
            .collect();
        Ok(cell_deps)
    }

    // header deps hashes
    let mut header_deps_hashes: Vec<H256> = Vec::with_capacity(
        tx.raw().header_deps().len() + tx.raw().inputs().len() + tx.raw().cell_deps().len(),
    );

    let mut inputs: Vec<ReprMockInput> = Vec::with_capacity(tx.raw().inputs().len());
    for input in tx.raw().inputs() {
        let input_cell = match rpc_client
            .get_cell(input.previous_output())
            .await?
            .and_then(|cell_status| cell_status.cell)
        {
            Some(cell) => Some(cell),
            None => {
                rpc_client
                    .get_cell_from_mempool(input.previous_output())
                    .await?
            }
        }
        .ok_or_else(|| anyhow!("can't find input cell"))?;

        let input_tx_hash = input.previous_output().tx_hash().unpack();
        let input_block_hash = match rpc_client.ckb.get_transaction_status(input_tx_hash).await? {
            Some(gw_jsonrpc_types::ckb_jsonrpc_types::Status::Committed) => {
                let block_hash = rpc_client
                    .ckb
                    .get_transaction_block_hash(input_tx_hash)
                    .await?;
                Some(block_hash.ok_or_else(|| anyhow!("not found input cell tx hash"))?)
            }
            _ => None,
        };
        let mock_input = ReprMockInput {
            input: {
                let ckb_input = ckb_types::packed::CellInput::new_unchecked(input.as_bytes());
                ckb_input.into()
            },
            output: {
                let ckb_output =
                    ckb_types::packed::CellOutput::new_unchecked(input_cell.output.as_bytes());
                ckb_output.into()
            },
            data: ckb_jsonrpc_types::JsonBytes::from_bytes(input_cell.data),
            header: input_block_hash.map(|h| h.into()),
        };
        inputs.push(mock_input);
        if let Some(input_block_hash) = input_block_hash {
            header_deps_hashes.push(input_block_hash);
        }
    }

    // resolve cell groups
    let mut resolved_cell_deps = Vec::with_capacity(tx.raw().cell_deps().len());
    for cell_dep in tx.raw().cell_deps() {
        let cell_deps = resolve_dep_group(rpc_client, &cell_dep).await?;
        resolved_cell_deps.push(cell_dep);
        resolved_cell_deps.extend(cell_deps);
    }

    let mut cell_deps: Vec<ReprMockCellDep> = Vec::with_capacity(resolved_cell_deps.len());
    for cell_dep in resolved_cell_deps {
        let dep_cell = match rpc_client
            .get_cell(cell_dep.out_point())
            .await?
            .and_then(|cell_status| cell_status.cell)
        {
            Some(cell) => Some(cell),
            None => {
                rpc_client
                    .get_cell_from_mempool(cell_dep.out_point())
                    .await?
            }
        }
        .ok_or_else(|| anyhow!("can't find dep cell"))?;
        let dep_cell_tx_hash = cell_dep.out_point().tx_hash().unpack();
        let dep_cell_block_hash = match rpc_client
            .ckb
            .get_transaction_status(dep_cell_tx_hash)
            .await?
        {
            Some(gw_jsonrpc_types::ckb_jsonrpc_types::Status::Committed) => {
                let query = rpc_client
                    .ckb
                    .get_transaction_block_hash(dep_cell_tx_hash)
                    .await?;
                Some(query.ok_or_else(|| anyhow!("not found dep cell tx hash"))?)
            }
            _ => None,
        };
        let mock_cell_dep = ReprMockCellDep {
            cell_dep: {
                let ckb_cell_dep = ckb_types::packed::CellDep::new_unchecked(cell_dep.as_bytes());
                ckb_cell_dep.into()
            },
            output: {
                let ckb_output =
                    ckb_types::packed::CellOutput::new_unchecked(dep_cell.output.as_bytes());
                ckb_output.into()
            },
            data: { ckb_jsonrpc_types::JsonBytes::from_bytes(dep_cell.data) },
            header: dep_cell_block_hash.map(|h| h.into()),
        };
        cell_deps.push(mock_cell_dep);
        if let Some(dep_cell_block_hash) = dep_cell_block_hash {
            header_deps_hashes.push(dep_cell_block_hash);
        }
    }

    header_deps_hashes.extend(
        tx.raw()
            .header_deps()
            .into_iter()
            .map(|h| Unpack::<H256>::unpack(&h)),
    );

    let mut header_deps: Vec<ckb_jsonrpc_types::HeaderView> =
        Vec::with_capacity(header_deps_hashes.len());

    for block_hash in header_deps_hashes {
        let header = rpc_client
            .get_header(block_hash)
            .await?
            .ok_or_else(|| anyhow!("block header not found"))?;
        header_deps.push(header);
    }

    let mock_info = ReprMockInfo {
        inputs,
        cell_deps,
        header_deps,
    };

    let mock_tx = ReprMockTransaction {
        mock_info,
        tx: {
            let ckb_tx = ckb_types::packed::Transaction::new_unchecked(tx.as_bytes());
            ckb_tx.into()
        },
    };
    Ok(mock_tx)
}
