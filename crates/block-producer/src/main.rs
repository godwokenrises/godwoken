use anyhow::{anyhow, Result};
use cell_collector::{CellCollector, DepositInfo};
use gw_block_producer::block_producer::{produce_block, ProduceBlockParam, ProduceBlockResult};
use gw_chain::chain::Chain;
use gw_common::H256;
use gw_config::Config;
use gw_generator::{
    account_lock_manage::AccountLockManage, backend_manage::BackendManage, genesis::init_genesis,
    Generator, RollupContext,
};
use gw_mem_pool::pool::MemPool;
use gw_store::Store;
use gw_types::{
    bytes::Bytes,
    packed::{CellDep, CellInput, CellOutput, GlobalState, L2Block, Transaction, WitnessArgs},
    prelude::{Builder, Entity, Pack},
};
use parking_lot::Mutex;
use std::{collections::HashSet, fs, path::Path, sync::Arc};
use transaction_skeleton::TransactionSkeleton;

mod block_producer;
mod cell_collector;
mod transaction_skeleton;

fn read_config<P: AsRef<Path>>(path: P) -> Result<Config> {
    let content = fs::read(path)?;
    let config = toml::from_slice(&content)?;
    Ok(config)
}

fn generate_custodian_cells(
    block: &L2Block,
    deposit_cells: &[DepositInfo],
) -> Vec<(CellOutput, Bytes)> {
    unimplemented!()
}

/// Add fee cell to tx skeleton
fn fill_tx_fee(collector: &CellCollector, tx_skeleton: &mut TransactionSkeleton) -> Result<()> {
    unimplemented!()
}

// sign message
fn sign(msg: [u8; 32]) -> [u8; 65] {
    unimplemented!()
}

fn build_tx(
    collector: &CellCollector,
    deposit_cells: Vec<DepositInfo>,
    block: L2Block,
    global_state: GlobalState,
) -> Result<Transaction> {
    let rollup_cell_info = collector
        .query_rollup_cell()
        .ok_or(anyhow!("can't find rollup cell"))?;
    let mut tx_skeleton = TransactionSkeleton::default();
    // rollup cell
    tx_skeleton.inputs().push(
        CellInput::new_builder()
            .previous_output(rollup_cell_info.out_point)
            .build(),
    );
    // deps
    tx_skeleton.cell_deps().push(
        rollup_cell_info
            .type_dep
            .ok_or(anyhow!("rollup type dep should exists"))?,
    );
    tx_skeleton.cell_deps().push(rollup_cell_info.lock_dep);
    // deposit lock dep
    if let Some(deposit) = deposit_cells.first() {
        tx_skeleton.cell_deps().push(deposit.cell.lock_dep.clone());
    }
    // witnesses
    tx_skeleton.witnesses().push(
        WitnessArgs::new_builder()
            .output_type(Some(block.as_bytes()).pack())
            .build(),
    );
    // output
    let output = rollup_cell_info.output;
    let output_data = global_state.as_bytes();
    tx_skeleton.outputs().push((output, output_data));
    // deposit cells
    for deposit in &deposit_cells {
        tx_skeleton.inputs().push(
            CellInput::new_builder()
                .previous_output(deposit.cell.out_point.clone())
                .build(),
        );
    }

    // Some deposition cells might have type scripts for sUDTs, handle cell deps
    // here.
    let deposit_type_deps: HashSet<CellDep> = deposit_cells
        .iter()
        .filter_map(|deposit| deposit.cell.type_dep.clone())
        .collect();
    tx_skeleton.cell_deps().extend(deposit_type_deps);
    // custodian cells
    let custodian_cells = generate_custodian_cells(&block, &deposit_cells);
    tx_skeleton.outputs().extend(custodian_cells);
    // TODO stake cell
    // tx fee cell
    fill_tx_fee(collector, &mut tx_skeleton)?;
    let mut signatures = Vec::new();
    for message in tx_skeleton.signature_messages() {
        signatures.push(sign(message));
    }
    let tx = tx_skeleton.seal(signatures)?;
    Ok(tx)
}

fn produce_next_block(
    config: &Config,
    collector: &CellCollector,
    chain: &Chain,
    rollup_config_hash: &H256,
    block_producer_id: u32,
    timestamp: u64,
) -> Result<()> {
    // get deposit cells
    let deposit_cells = collector.query_deposit_cells();

    // get txs & withdrawal requests from mem pool
    let mut txs = Vec::new();
    let mut withdrawal_requests = Vec::new();
    {
        let mem_pool = chain.mem_pool.lock();
        for (_id, entry) in mem_pool.pending() {
            if let Some(withdrawal) = entry.withdrawals.first() {
                withdrawal_requests.push(withdrawal.clone());
            } else {
                txs.extend(entry.txs.iter().cloned());
            }
        }
    };
    let parent_block = chain.local_state.tip();
    let max_withdrawal_capacity = std::u128::MAX;
    // produce block
    let param = ProduceBlockParam {
        db: chain.store.begin_transaction(),
        generator: &chain.generator,
        block_producer_id,
        timestamp,
        txs,
        deposition_requests: deposit_cells.iter().map(|d| &d.request).cloned().collect(),
        withdrawal_requests,
        parent_block,
        rollup_config_hash,
        max_withdrawal_capacity,
    };
    let block_result = produce_block(param)?;
    let ProduceBlockResult {
        block,
        global_state,
        unused_transactions,
        unused_withdrawal_requests,
    } = block_result;
    println!(
        "produce new block {} unused transactions {} unused withdrawals {}",
        block.raw().number(),
        unused_transactions.len(),
        unused_withdrawal_requests.len()
    );
    let block_hash = block.hash().into();

    // composit tx
    let tx = build_tx(&collector, deposit_cells, block, global_state)?;
    collector.send_transaction(tx)?;

    // update status
    chain.mem_pool.lock().notify_new_tip(block_hash)?;
    Ok(())
}

fn run() -> Result<()> {
    let config_path = "./config.toml";
    // read config
    let config = read_config(&config_path)?;
    // start godwoken components
    // TODO: use persistent store later
    let store = Store::open_tmp()?;
    init_genesis(
        &store,
        &config.genesis,
        config.rollup_deployment.genesis_header.clone().into(),
    )?;
    let rollup_context = RollupContext {
        rollup_config: config.genesis.rollup_config.clone().into(),
        rollup_script_hash: {
            let rollup_script_hash: [u8; 32] = config.genesis.rollup_script_hash.clone().into();
            rollup_script_hash.into()
        },
    };

    let rollup_config_hash = rollup_context.rollup_config.hash().into();
    let generator = {
        let backend_manage = BackendManage::from_config(config.backends.clone())?;
        let account_lock_manage = AccountLockManage::default();
        Arc::new(Generator::new(
            backend_manage,
            account_lock_manage,
            rollup_context,
        ))
    };
    let mem_pool = Arc::new(Mutex::new(MemPool::create(
        store.clone(),
        generator.clone(),
    )?));
    let chain = Chain::create(
        config.chain.clone(),
        store.clone(),
        generator.clone(),
        mem_pool.clone(),
    )?;
    // query parameters
    let block_producer_id = 0;
    let timestamp = 0;
    let collector = CellCollector;

    // produce block
    produce_next_block(
        &config,
        &collector,
        &chain,
        &rollup_config_hash,
        block_producer_id,
        timestamp,
    )?;

    Ok(())
}

/// Block producer
fn main() {
    run().expect("block producer");
}
