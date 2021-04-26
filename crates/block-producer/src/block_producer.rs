#![allow(clippy::clippy::mutable_key_type)]

use crate::{
    poa::PoA,
    produce_block::{produce_block, ProduceBlockParam, ProduceBlockResult},
    rpc_client::{DepositInfo, RPCClient},
    transaction_skeleton::TransactionSkeleton,
    types::ChainEvent,
    types::{CellInfo, InputCellInfo},
    utils::{fill_tx_fee, CKBGenesisInfo},
    wallet::Wallet,
};
use anyhow::{anyhow, Context, Result};
use ckb_types::prelude::Unpack as CKBUnpack;
use futures::{future::select_all, FutureExt};
use gw_chain::chain::Chain;
use gw_common::H256;
use gw_config::BlockProducerConfig;
use gw_generator::{Generator, RollupContext};
use gw_mem_pool::pool::MemPool;
use gw_store::Store;
use gw_types::{
    bytes::Bytes,
    core::{DepType, ScriptHashType},
    packed::{
        Byte32, CellDep, CellInput, CellOutput, CustodianLockArgs, DepositionLockArgs, GlobalState,
        L2Block, OutPoint, OutPointVec, Script, Transaction, WitnessArgs,
    },
    prelude::*,
};
use parking_lot::Mutex;
use std::{
    collections::{HashMap, HashSet},
    convert::TryFrom,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

fn generate_custodian_cells(
    rollup_context: &RollupContext,
    block: &L2Block,
    deposit_cells: &[DepositInfo],
) -> Vec<(CellOutput, Bytes)> {
    let block_hash: Byte32 = block.hash().pack();
    let block_number = block.raw().number();
    deposit_cells
        .iter()
        .map(|deposit_info| {
            let lock_args = {
                let deposition_lock_args = DepositionLockArgs::new_unchecked(
                    deposit_info.cell.output.lock().args().unpack(),
                );

                CustodianLockArgs::new_builder()
                    .deposition_block_hash(block_hash.clone())
                    .deposition_block_number(block_number.clone())
                    .deposition_lock_args(deposition_lock_args)
                    .build()
            };
            let lock = Script::new_builder()
                .code_hash(rollup_context.rollup_config.custodian_script_type_hash())
                .hash_type(ScriptHashType::Type.into())
                .args(lock_args.as_bytes().pack())
                .build();

            // use custodian lock
            let cell = deposit_info
                .cell
                .output
                .clone()
                .as_builder()
                .lock(lock)
                .build();
            let data = deposit_info.cell.data.clone();
            (cell, data)
        })
        .collect()
}

async fn resolve_tx_deps(rpc_client: &RPCClient, tx_hash: [u8; 32]) -> Result<Vec<CellInfo>> {
    async fn resolve_dep_group(rpc_client: &RPCClient, dep: CellDep) -> Result<Vec<CellDep>> {
        // return dep
        if dep.dep_type() == DepType::Code.into() {
            return Ok(vec![dep]);
        }
        // parse dep group
        let cell = rpc_client
            .get_cell(dep.out_point())
            .await?
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

    // get deposit cells txs
    let tx = rpc_client
        .get_transaction(tx_hash.into())
        .await?
        .ok_or_else(|| anyhow!("can't get deposit tx"))?;
    let mut resolve_dep_futs: Vec<_> = tx
        .raw()
        .cell_deps()
        .into_iter()
        .map(|dep| resolve_dep_group(rpc_client, dep).boxed())
        .collect();
    let mut get_cell_futs = Vec::default();

    // wait resolved dep groups futures
    while !resolve_dep_futs.is_empty() {
        let (tx_cell_deps_res, _index, remained) = select_all(resolve_dep_futs.into_iter()).await;
        resolve_dep_futs = remained;
        let tx_cell_deps = tx_cell_deps_res?;
        let futs = tx_cell_deps
            .iter()
            .map(|dep| rpc_client.get_cell(dep.out_point()).boxed());
        get_cell_futs.extend(futs);
    }

    // wait all cells
    let mut cells = Vec::with_capacity(get_cell_futs.len());
    for cell_fut in get_cell_futs {
        let cell = cell_fut
            .await?
            .ok_or_else(|| anyhow!("can't find dep cell"))?;
        cells.push(cell);
    }
    Ok(cells)
}

pub struct BlockProducer {
    rollup_config_hash: H256,
    store: Store,
    chain: Arc<Mutex<Chain>>,
    mem_pool: Arc<Mutex<MemPool>>,
    generator: Arc<Generator>,
    poa: PoA,
    wallet: Wallet,
    config: BlockProducerConfig,
    rpc_client: RPCClient,
    ckb_genesis_info: CKBGenesisInfo,
}

impl BlockProducer {
    #[allow(clippy::too_many_arguments)]
    pub fn create(
        rollup_config_hash: H256,
        store: Store,
        generator: Arc<Generator>,
        chain: Arc<Mutex<Chain>>,
        mem_pool: Arc<Mutex<MemPool>>,
        rpc_client: RPCClient,
        ckb_genesis_info: CKBGenesisInfo,
        config: BlockProducerConfig,
    ) -> Result<Self> {
        let wallet = Wallet::from_config(&config.wallet_config).with_context(|| "init wallet")?;
        let poa = PoA::new(
            rpc_client.clone(),
            wallet.lock().clone(),
            config.poa_lock_dep.clone().into(),
            config.poa_state_dep.clone().into(),
        );

        let block_producer = BlockProducer {
            rollup_config_hash,
            store,
            generator,
            chain,
            mem_pool,
            rpc_client,
            wallet,
            poa,
            ckb_genesis_info,
            config,
        };
        Ok(block_producer)
    }

    pub async fn handle_event(&mut self, event: ChainEvent) -> Result<()> {
        // assume the chain is updated
        let tip_block = match event {
            ChainEvent::Reverted {
                old_tip: _,
                new_block,
            } => new_block,
            ChainEvent::NewBlock { block } => block,
        };
        let header = tip_block.header();
        let tip_hash: H256 = header.hash().into();

        // query median time & rollup cell
        let rollup_cell_opt = self.rpc_client.query_rollup_cell().await?;
        let median_time = self.rpc_client.get_block_median_time(tip_hash).await?;
        // let (rollup_cell_opt, median_time) = futures::try_join!(rollup_cell_fut, median_time_fut)?;
        let rollup_cell = rollup_cell_opt.ok_or(anyhow!("can't found rollup cell"))?;
        let poa_cell_input = InputCellInfo {
            input: CellInput::new_builder()
                .previous_output(rollup_cell.out_point.clone())
                .build(),
            cell: rollup_cell.clone(),
        };

        // try issue next block
        if self
            .poa
            .should_issue_next_block(median_time, &poa_cell_input)
            .await?
        {
            self.produce_next_block(median_time, rollup_cell).await?;
        }
        Ok(())
    }

    pub async fn produce_next_block(
        &mut self,
        median_time: Duration,
        rollup_cell: CellInfo,
    ) -> Result<()> {
        let block_producer_id = self.config.account_id;
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("timestamp")
            .as_millis() as u64;

        // get deposit cells
        let deposit_cells = self.rpc_client.query_deposit_cells().await?;

        // get txs & withdrawal requests from mem pool
        let mut txs = Vec::new();
        let mut withdrawal_requests = Vec::new();
        {
            let mem_pool = self.mem_pool.lock();
            for entry in mem_pool.pending().values() {
                if let Some(withdrawal) = entry.withdrawals.first() {
                    withdrawal_requests.push(withdrawal.clone());
                } else {
                    txs.extend(entry.txs.iter().cloned());
                }
            }
        };
        let parent_block = self.chain.lock().local_state().tip().clone();
        let max_withdrawal_capacity = std::u128::MAX;
        // produce block
        let param = ProduceBlockParam {
            db: self.store.begin_transaction(),
            generator: &self.generator,
            block_producer_id,
            timestamp,
            txs,
            deposition_requests: deposit_cells.iter().map(|d| &d.request).cloned().collect(),
            withdrawal_requests,
            parent_block: &parent_block,
            rollup_config_hash: &self.rollup_config_hash,
            max_withdrawal_capacity,
        };
        let block_result = produce_block(param)?;
        let ProduceBlockResult {
            block,
            global_state,
            unused_transactions,
            unused_withdrawal_requests,
        } = block_result;
        let number: u64 = block.raw().number().unpack();
        println!(
            "produce new block #{} (txs: {}, deposits: {}, staled txs: {}, staled withdrawals: {})",
            number,
            block.transactions().len(),
            deposit_cells.len(),
            unused_transactions.len(),
            unused_withdrawal_requests.len()
        );

        // composit tx
        let tx = self
            .complete_tx_skeleton(deposit_cells, block, global_state, median_time, rollup_cell)
            .await?;

        // send transaction
        match self.rpc_client.send_transaction(tx).await {
            Ok(tx_hash) => {
                println!("Submitted l2 block in {:?}", tx_hash);
            }
            Err(err) => {
                eprintln!("Submitting l2 block error: {}", err);
                self.poa.reset_current_round();
            }
        }
        Ok(())
    }

    async fn complete_tx_skeleton(
        &self,
        deposit_cells: Vec<DepositInfo>,
        block: L2Block,
        global_state: GlobalState,
        median_time: Duration,
        rollup_cell: CellInfo,
    ) -> Result<Transaction> {
        let rollup_context = self.generator.rollup_context();
        let mut tx_skeleton = TransactionSkeleton::default();
        let rollup_cell_input_index = tx_skeleton.inputs().len();
        // rollup cell
        tx_skeleton.inputs_mut().push(InputCellInfo {
            input: CellInput::new_builder()
                .previous_output(rollup_cell.out_point.clone())
                .build(),
            cell: rollup_cell.clone(),
        });
        // rollup deps
        tx_skeleton
            .cell_deps_mut()
            .push(self.config.rollup_cell_type_dep.clone().into());
        // deposit lock dep
        if !deposit_cells.is_empty() {
            let cell_dep: CellDep = self.config.deposit_cell_lock_dep.clone().into();
            tx_skeleton
                .cell_deps_mut()
                .push(CellDep::new_unchecked(cell_dep.as_bytes()));
        }
        // secp256k1 lock, used for unlock tx fee payment cells
        tx_skeleton
            .cell_deps_mut()
            .push(self.ckb_genesis_info.sighash_dep());
        // witnesses
        tx_skeleton.witnesses_mut().push(
            WitnessArgs::new_builder()
                .output_type(Some(block.as_bytes()).pack())
                .build(),
        );
        // output
        let output = rollup_cell.output;
        let output_data = global_state.as_bytes();
        tx_skeleton.outputs_mut().push((output, output_data));
        // deposit cells
        for deposit in &deposit_cells {
            let input = CellInput::new_builder()
                .previous_output(deposit.cell.out_point.clone())
                .build();
            tx_skeleton.inputs_mut().push(InputCellInfo {
                input,
                cell: deposit.cell.clone(),
            });
        }

        // Some deposition cells might have type scripts for sUDTs, handle cell deps
        // here.
        let deposit_type_deps: HashSet<CellDep> = {
            // fetch deposit cells deps
            let dep_cell_futs: Vec<_> = deposit_cells
                .iter()
                .filter_map(|deposit| {
                    deposit.cell.output.type_().to_opt().map(|_type_| {
                        resolve_tx_deps(&self.rpc_client, deposit.cell.out_point.tx_hash().unpack())
                    })
                })
                .collect();

            // wait futures
            let mut dep_cells: Vec<CellInfo> = Vec::new();
            for fut in dep_cell_futs {
                dep_cells.extend(fut.await?);
            }

            // resolve deposit cells deps
            let dep_cell_by_data: HashMap<[u8; 32], OutPoint> = dep_cells
                .iter()
                .map(|cell| {
                    let data_hash =
                        ckb_types::packed::CellOutput::calc_data_hash(&cell.data).unpack();
                    (data_hash, cell.out_point.clone())
                })
                .collect();
            let dep_cell_by_type: HashMap<[u8; 32], OutPoint> = dep_cells
                .iter()
                .filter_map(|cell| {
                    cell.output
                        .type_()
                        .to_opt()
                        .map(|type_| (type_.hash(), cell.out_point.clone()))
                })
                .collect();

            let mut deps: HashSet<CellDep> = Default::default();
            for deposit in &deposit_cells {
                if let Some(type_) = deposit.cell.output.type_().to_opt() {
                    let code_hash: [u8; 32] = type_.code_hash().unpack();
                    let out_point_opt = match ScriptHashType::try_from(type_.hash_type())
                        .map_err(|n| anyhow!("invalid hash_type {}", n))?
                    {
                        ScriptHashType::Data => dep_cell_by_data.get(&code_hash),
                        ScriptHashType::Type => dep_cell_by_type.get(&code_hash),
                    };
                    let out_point = out_point_opt
                        .ok_or_else(|| anyhow!("can't find deps code_hash: {:?}", code_hash))?;
                    let cell_dep = CellDep::new_builder()
                        .out_point(out_point.to_owned())
                        .dep_type(DepType::Code.into())
                        .build();
                    deps.insert(cell_dep);
                }
            }
            deps
        };
        tx_skeleton.cell_deps_mut().extend(deposit_type_deps);
        // custodian cells
        let custodian_cells = generate_custodian_cells(rollup_context, &block, &deposit_cells);
        tx_skeleton.outputs_mut().extend(custodian_cells);
        self.poa
            .fill_poa(&mut tx_skeleton, rollup_cell_input_index, median_time)
            .await?;
        println!(
            "tx outputs : {:?}",
            tx_skeleton
                .outputs()
                .iter()
                .enumerate()
                .map(|(i, (o, d))| {
                    let c: u64 = o.capacity().unpack();
                    (i, c, d.len())
                })
                .collect::<Vec<_>>()
        );
        // TODO stake cell
        // tx fee cell
        fill_tx_fee(
            &mut tx_skeleton,
            &self.rpc_client,
            self.wallet.lock().to_owned(),
        )
        .await?;
        debug_assert_eq!(
            tx_skeleton.taken_outpoints()?.len(),
            tx_skeleton.inputs().len(),
            "check duplicated inputs"
        );
        // sign
        let tx = self.wallet.sign_tx_skeleton(tx_skeleton)?;
        eprintln!("final tx size: {}", tx.as_slice().len());
        Ok(tx)
    }
}
