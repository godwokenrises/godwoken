use anyhow::Result;
use cell_collector::CellCollector;
use gw_block_producer::block_producer::{produce_block, ProduceBlockParam, ProduceBlockResult};
use gw_chain::chain::Chain;
use gw_config::Config;
use gw_generator::{
    account_lock_manage::AccountLockManage, backend_manage::BackendManage, genesis::init_genesis,
    Generator, RollupContext,
};
use gw_mem_pool::pool::MemPool;
use gw_store::Store;
use gw_types::{
    packed::{CellInput, DepositionRequest, OutPoint, RawTransaction, Transaction, WitnessArgs},
    prelude::{Builder, Entity, Pack},
};
use parking_lot::Mutex;
use std::sync::Arc;
use transaction_skeleton::TransactionSkeleton;

mod block_producer;
mod cell_collector;
mod transaction_skeleton;

fn read_config() -> Result<Config> {
    unimplemented!()
}

/// Block producer
fn main() {
    // read config
    let config = read_config().expect("read config");
    // start godwoken components
    // TODO: use persistent store later
    let store = Store::open_tmp().expect("store");
    init_genesis(
        &store,
        &config.genesis,
        config.rollup_deployment.genesis_header,
    )
    .expect("init genesis");
    let rollup_context = RollupContext {
        rollup_config: config.genesis.rollup_config.clone(),
        rollup_script_hash: {
            let rollup_script_hash: [u8; 32] = config.genesis.rollup_script_hash.clone().into();
            rollup_script_hash.into()
        },
    };
    let generator = {
        let backend_manage = BackendManage::from_config(config.backends).expect("backend manage");
        let account_lock_manage = AccountLockManage::default();
        Arc::new(Generator::new(
            backend_manage,
            account_lock_manage,
            rollup_context,
        ))
    };
    let mem_pool = Arc::new(Mutex::new(
        MemPool::create(store.clone(), generator.clone()).expect("mem pool"),
    ));
    let chain = Chain::create(
        config.chain.clone(),
        store.clone(),
        generator.clone(),
        mem_pool.clone(),
    )
    .expect("Error creating chain");
    // query parameters
    let block_producer_id = 0;
    let timestamp = 0;
    let collector = CellCollector;
    let deposition_requests = collector.query_deposition_requests();
    let has_deposit = !deposition_requests.is_empty();

    // get txs & withdrawal requests from mem pool
    let mut txs = Vec::new();
    let mut withdrawal_requests = Vec::new();
    {
        let mem_pool = mem_pool.lock();
        for (_id, entry) in mem_pool.pending() {
            if let Some(withdrawal) = entry.withdrawals.first() {
                withdrawal_requests.push(withdrawal.clone());
            } else {
                txs.extend(entry.txs.iter().cloned());
            }
        }
    };
    let parent_block = chain.local_state.tip();
    let rollup_config_hash = config.genesis.rollup_config.hash().into();
    let max_withdrawal_capacity = std::u128::MAX;
    // produce block
    let param = ProduceBlockParam {
        db: store.begin_transaction(),
        generator: &generator,
        block_producer_id,
        timestamp,
        txs,
        deposition_requests,
        withdrawal_requests,
        parent_block,
        rollup_config_hash: &rollup_config_hash,
        max_withdrawal_capacity,
    };
    let block_result = produce_block(param).expect("produce block");
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

    // composit tx
    let rollup_cell_info = collector.query_rollup_cell().expect("rollup cell");
    let mut tx_skeleton = TransactionSkeleton::default();
    // rollup cell
    tx_skeleton.inputs().push(
        CellInput::new_builder()
            .previous_output(rollup_cell_info.out_point)
            .build(),
    );
    // deps
    tx_skeleton
        .cell_deps()
        .push(config.rollup_deployment.rollup_type_dep);
    tx_skeleton
        .cell_deps()
        .push(config.rollup_deployment.rollup_lock_dep);
    if has_deposit {
        tx_skeleton
            .cell_deps()
            .push(config.rollup_deployment.deposit_lock_dep);
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

    // update status
    mem_pool
        .lock()
        .notify_new_tip(block.hash().into())
        .expect("update mem pool status");
}
