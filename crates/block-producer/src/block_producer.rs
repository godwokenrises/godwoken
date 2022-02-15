#![allow(clippy::mutable_key_type)]

use crate::{
    custodian::query_mergeable_custodians,
    produce_block::{
        generate_produce_block_param, produce_block, ProduceBlockParam, ProduceBlockResult,
    },
    replay_block::ReplayBlock,
    test_mode_control::TestModeControl,
    types::ChainEvent,
    utils,
};
use anyhow::{anyhow, bail, Context, Result};
use ckb_chain_spec::consensus::MAX_BLOCK_BYTES;
use ckb_types::prelude::Unpack as CKBUnpack;
use futures::{future::select_all, FutureExt};
use gw_chain::chain::{Chain, SyncEvent};
use gw_common::{h256_ext::H256Ext, H256};
use gw_config::{BlockProducerConfig, DebugConfig};
use gw_generator::Generator;
use gw_jsonrpc_types::test_mode::TestModePayload;
use gw_mem_pool::{
    custodian::to_custodian_cell,
    pool::{MemPool, OutputParam},
};
use gw_rpc_client::{contract::ContractsCellDepManager, rpc_client::RPCClient};
use gw_store::{traits::chain_store::ChainStore, Store};
use gw_types::{
    bytes::Bytes,
    core::{DepType, ScriptHashType, Status},
    offchain::{
        global_state_from_slice, CellInfo, CollectedCustodianCells, DepositInfo, InputCellInfo,
        RollupContext,
    },
    packed::{
        CellDep, CellInput, CellOutput, GlobalState, L2Block, OutPoint, OutPointVec, RollupAction,
        RollupActionUnion, RollupSubmitBlock, Transaction, WithdrawalRequestExtra, WitnessArgs,
    },
    prelude::*,
};
use gw_utils::{
    fee::fill_tx_fee, genesis_info::CKBGenesisInfo, transaction_skeleton::TransactionSkeleton,
    wallet::Wallet,
};
use std::{
    collections::{HashMap, HashSet},
    convert::TryFrom,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::Mutex;

const MAX_BLOCK_OUTPUT_PARAM_RETRY_COUNT: usize = 10;
const TRANSACTION_SCRIPT_ERROR: &str = "TransactionScriptError";
const TRANSACTION_EXCEEDED_MAXIMUM_BLOCK_BYTES_ERROR: &str = "ExceededMaximumBlockBytes";
const TRANSACTION_FAILED_TO_RESOLVE_ERROR: &str = "TransactionFailedToResolve";
/// 524_288 we choose this value because it is smaller than the MAX_BLOCK_BYTES which is 597K
const MAX_ROLLUP_WITNESS_SIZE: usize = 1 << 19;
const WAIT_PRODUCE_BLOCK_SECONDS: u64 = 90;

enum SubmitResult {
    Submitted,
    Skip,
}

fn generate_custodian_cells(
    rollup_context: &RollupContext,
    block: &L2Block,
    deposit_cells: &[DepositInfo],
) -> Vec<(CellOutput, Bytes)> {
    let block_hash: H256 = block.hash().into();
    let block_number = block.raw().number().unpack();
    let to_custodian = |deposit| -> _ {
        to_custodian_cell(rollup_context, &block_hash, block_number, deposit)
            .expect("sanitized deposit")
    };

    deposit_cells.iter().map(to_custodian).collect()
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
            .and_then(|cell_status| cell_status.cell)
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
            .and_then(|cell_status| cell_status.cell)
            .ok_or_else(|| anyhow!("can't find dep cell"))?;
        cells.push(cell);
    }
    Ok(cells)
}

struct LastCommittedL2Block {
    committed_at: Instant,
    committed_tip_block_hash: H256,
}

pub struct BlockProducer {
    rollup_config_hash: H256,
    store: Store,
    chain: Arc<Mutex<Chain>>,
    mem_pool: Arc<Mutex<MemPool>>,
    generator: Arc<Generator>,
    wallet: Wallet,
    config: BlockProducerConfig,
    debug_config: DebugConfig,
    rpc_client: RPCClient,
    ckb_genesis_info: CKBGenesisInfo,
    tests_control: Option<TestModeControl>,
    last_committed_l2_block: LastCommittedL2Block,
    last_submitted_tx_hash: Arc<tokio::sync::RwLock<H256>>,
    contracts_dep_manager: ContractsCellDepManager,
}

pub struct BlockProducerCreateArgs {
    pub rollup_config_hash: H256,
    pub store: Store,
    pub generator: Arc<Generator>,
    pub chain: Arc<Mutex<Chain>>,
    pub mem_pool: Arc<Mutex<MemPool>>,
    pub rpc_client: RPCClient,
    pub ckb_genesis_info: CKBGenesisInfo,
    pub config: BlockProducerConfig,
    pub debug_config: DebugConfig,
    pub tests_control: Option<TestModeControl>,
    pub contracts_dep_manager: ContractsCellDepManager,
}

impl BlockProducer {
    pub fn create(args: BlockProducerCreateArgs) -> Result<Self> {
        let BlockProducerCreateArgs {
            rollup_config_hash,
            store,
            generator,
            chain,
            mem_pool,
            rpc_client,
            ckb_genesis_info,
            config,
            debug_config,
            tests_control,
            contracts_dep_manager,
        } = args;

        let wallet = Wallet::from_config(&config.wallet_config).with_context(|| "init wallet")?;

        let block_producer = BlockProducer {
            rollup_config_hash,
            generator,
            chain,
            mem_pool,
            rpc_client,
            wallet,
            ckb_genesis_info,
            config,
            debug_config,
            tests_control,
            last_committed_l2_block: LastCommittedL2Block {
                committed_at: Instant::now(),
                committed_tip_block_hash: H256::zero(),
            },
            last_submitted_tx_hash: {
                let tip_block_hash = store.get_tip_block_hash()?;
                let committed_info = store
                    .get_l2block_committed_info(&tip_block_hash)?
                    .ok_or_else(|| anyhow!("can't find committed info for tip block"))?;
                Arc::new(tokio::sync::RwLock::new(
                    committed_info.transaction_hash().unpack(),
                ))
            },
            store,
            contracts_dep_manager,
        };
        Ok(block_producer)
    }

    pub fn last_submitted_tx_hash(&self) -> Arc<tokio::sync::RwLock<H256>> {
        self.last_submitted_tx_hash.clone()
    }

    pub async fn handle_event(&mut self, event: ChainEvent) -> Result<()> {
        if let Some(ref tests_control) = self.tests_control {
            match tests_control.payload().await {
                Some(TestModePayload::Challenge { .. }) // Payload not match
                | Some(TestModePayload::WaitForChallengeMaturity) // Payload not match
                | None => return Ok(()), // Wait payload
                Some(TestModePayload::None) // Produce block
                | Some(TestModePayload::BadBlock { .. }) => (), // Produce block but create bad block
            }
        }

        let last_sync_event = { self.chain.lock().await.last_sync_event().to_owned() };
        match last_sync_event {
            SyncEvent::Success => (),
            _ => return Ok(()),
        }

        // check l2 tip
        let l2_tip_block_hash = self
            .store
            .begin_transaction()
            .get_last_valid_tip_block_hash()?;

        // skip produce new block unless:
        // 1. local l2 tip updated
        // 2. wait produce block seconds
        if l2_tip_block_hash == self.last_committed_l2_block.committed_tip_block_hash
            && self
                .last_committed_l2_block
                .committed_at
                .elapsed()
                .as_secs()
                < WAIT_PRODUCE_BLOCK_SECONDS
        {
            log::debug!(
                "skip producing new block, last committed is {}s ago",
                self.last_committed_l2_block
                    .committed_at
                    .elapsed()
                    .as_secs()
            );
            return Ok(());
        }

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
        let rollup_cell = rollup_cell_opt.ok_or_else(|| anyhow!("can't found rollup cell"))?;
        let global_state = global_state_from_slice(&rollup_cell.data)?;
        let rollup_state = {
            let status: u8 = global_state.status().into();
            Status::try_from(status).map_err(|n| anyhow!("invalid status {}", n))?
        };
        if Status::Halting == rollup_state {
            return Ok(());
        }

        let median_time = match self.rpc_client.get_block_median_time(tip_hash).await? {
            Some(time) => time,
            None => return Ok(()),
        };

        let mem_block_timestamp = {
            let mem_pool = self.mem_pool.lock().await;
            mem_pool.mem_block().block_info().timestamp().unpack()
        };
        if (median_time.as_millis() as u64) < mem_block_timestamp {
            // Wait next l1 tip block median time
            return Ok(());
        }

        // try issue next block
        let mut retry_count = 0;
        while retry_count <= MAX_BLOCK_OUTPUT_PARAM_RETRY_COUNT {
            let (block_number, tx) = match self
                .compose_next_block_submit_tx(median_time, rollup_cell.clone(), retry_count)
                .await
            {
                Ok((block_number, tx)) => (block_number, tx),
                Err(err) => {
                    retry_count += 1;
                    log::warn!(
                        "[produce block] retry compose next block submit tx, retry: {}, reason: {}",
                        retry_count,
                        err
                    );
                    continue;
                }
            };

            let expected_next_block_number = global_state.block().count().unpack();
            if expected_next_block_number != block_number {
                log::warn!("produce unexpected next block, expect {} produce {}, wait until chain is synced to latest block", expected_next_block_number, block_number);
                return Ok(());
            }

            let submitted_tx_hash = tx.hash();
            match self.submit_block_tx(block_number, tx).await {
                Ok(SubmitResult::Submitted) => {
                    self.last_committed_l2_block = LastCommittedL2Block {
                        committed_tip_block_hash: l2_tip_block_hash,
                        committed_at: Instant::now(),
                    };
                    let mut last_submitted_tx_hash = self.last_submitted_tx_hash.write().await;
                    *last_submitted_tx_hash = submitted_tx_hash.into();
                }
                Ok(SubmitResult::Skip) => {}
                Err(err) => {
                    retry_count += 1;
                    log::warn!(
                        "[produce block] retry submit block tx , retry: {}, reason: {}",
                        retry_count,
                        err
                    );
                    continue;
                }
            }

            return Ok(());
        }

        return Err(anyhow!(
            "[produce_next_block] produce block reach max retry"
        ));
    }

    async fn compose_next_block_submit_tx(
        &mut self,
        median_time: Duration,
        rollup_cell: CellInfo,
        retry_count: usize,
    ) -> Result<(u64, Transaction)> {
        if let Some(ref tests_control) = self.tests_control {
            match tests_control.payload().await {
                Some(TestModePayload::None) => tests_control.clear_none().await?,
                Some(TestModePayload::BadBlock { .. }) => (),
                _ => unreachable!(),
            }
        }

        // get txs & withdrawal requests from mem pool
        let (opt_finalized_custodians, block_param) = {
            let (mem_block, post_block_state) = {
                let t = Instant::now();
                log::debug!("[compose_next_block_submit_tx] acquire mem-pool",);
                let mem_pool = self.mem_pool.lock().await;
                log::debug!(
                    "[compose_next_block_submit_tx] unlock mem-pool {}ms",
                    t.elapsed().as_millis()
                );
                mem_pool.output_mem_block(&OutputParam::new(retry_count))
            };

            let t = Instant::now();
            let tip_block_number = mem_block.block_info().number().unpack().saturating_sub(1);
            let (finalized_custodians, produce_block_param) =
                generate_produce_block_param(&self.store, mem_block, post_block_state)?;
            let finalized_custodians = {
                let last_finalized_block_number = {
                    let context = self.generator.rollup_context();
                    context.last_finalized_block_number(tip_block_number)
                };
                let query = query_mergeable_custodians(
                    &self.rpc_client,
                    finalized_custodians.unwrap_or_default(),
                    last_finalized_block_number,
                );
                query.await?.expect_any()
            };
            log::debug!(
                "finalized custodians {:?}",
                finalized_custodians.cells_info.len()
            );

            log::debug!(
                "[compose_next_block_submit_tx] generate produce block param {}ms",
                t.elapsed().as_millis()
            );
            (Some(finalized_custodians), produce_block_param)
        };
        let deposit_cells = block_param.deposits.clone();
        let withdrawals = block_param.withdrawals.clone();

        // produce block
        let reverted_block_root: H256 = {
            let db = self.store.begin_transaction();
            let smt = db.reverted_block_smt()?;
            smt.root().to_owned()
        };
        let param = ProduceBlockParam {
            stake_cell_owner_lock_hash: self.wallet.lock_script().hash().into(),
            reverted_block_root,
            rollup_config_hash: self.rollup_config_hash,
            block_param,
        };
        let db = self.store.begin_transaction();
        let block_result = produce_block(&db, &self.generator, param)?;
        let ProduceBlockResult {
            mut block,
            mut global_state,
            withdrawal_extras,
        } = block_result;

        let number: u64 = block.raw().number().unpack();
        if self.config.check_mem_block_before_submit {
            let deposit_requests: Vec<_> =
                deposit_cells.iter().map(|i| i.request.clone()).collect();
            if let Err(err) = ReplayBlock::replay(
                &self.store,
                &self.generator,
                &block,
                &deposit_requests,
                &withdrawals,
            ) {
                let mut mem_pool = self.mem_pool.lock().await;
                mem_pool.save_mem_block_with_suffix(&format!("invalid_block_{}", number))?;
                bail!("replay block {} {}", number, err);
            }
        }

        let block_txs = block.transactions().len();
        let block_withdrawals = block.withdrawals().len();
        log::info!(
            "produce new block #{} (txs: {}, deposits: {}, withdrawals: {})",
            number,
            block_txs,
            deposit_cells.len(),
            block_withdrawals,
        );
        if !block.withdrawals().is_empty() && opt_finalized_custodians.is_none() {
            bail!("unexpected none custodians for withdrawals");
        }
        let finalized_custodians = opt_finalized_custodians.unwrap_or_default();

        if let Some(ref tests_control) = self.tests_control {
            if let Some(TestModePayload::BadBlock { .. }) = tests_control.payload().await {
                let (bad_block, bad_global_state) = tests_control
                    .generate_a_bad_block(block, global_state)
                    .await?;

                block = bad_block;
                global_state = bad_global_state;
            }
        }

        // composite tx
        let t = Instant::now();
        let args = CompleteTxArgs {
            deposit_cells,
            finalized_custodians,
            block,
            global_state,
            median_time,
            rollup_cell: rollup_cell.clone(),
            withdrawal_extras,
        };
        let tx = match self.complete_tx_skeleton(args).await {
            Ok(tx) => tx,
            Err(err) => {
                log::error!(
                    "[produce_next_block] Failed to composite submitting transaction: {}",
                    err
                );
                return Err(err);
            }
        };
        log::debug!(
            "[compose_next_block_submit_tx] complete tx skeleton {}ms",
            t.elapsed().as_millis()
        );
        if tx.as_slice().len() <= MAX_BLOCK_BYTES as usize
            && tx
                .witnesses()
                .get(0)
                .expect("rollup action")
                .as_slice()
                .len()
                <= MAX_ROLLUP_WITNESS_SIZE
        {
            Ok((number, tx))
        } else {
            utils::dump_transaction(&self.debug_config.debug_tx_dump_path, &self.rpc_client, &tx)
                .await;
            Err(anyhow!(
                "l2 block submit tx exceeded max block bytes, tx size: {} max block bytes: {}",
                tx.as_slice().len(),
                MAX_BLOCK_BYTES
            ))
        }
    }

    async fn submit_block_tx(
        &mut self,
        block_number: u64,
        tx: Transaction,
    ) -> Result<SubmitResult> {
        let t = Instant::now();
        let cycles = match self.rpc_client.dry_run_transaction(&tx).await {
            Ok(cycles) => {
                log::info!(
                    "Tx({}) L2 block #{} execution cycles: {}",
                    block_number,
                    hex::encode(tx.hash()),
                    cycles
                );
                cycles
            }
            Err(err) => {
                let err_str = err.to_string();
                if err_str.contains(TRANSACTION_FAILED_TO_RESOLVE_ERROR) {
                    // TODO: check dead out point
                    if let Err(err) = self.contracts_dep_manager.refresh().await {
                        // Lets retry on next error
                        log::error!("[contracts dep] refresh failed {}", err);
                    }

                    log::info!("Skip submitting l2 block since CKB can't resolve tx, previous block may haven't been processed by CKB");
                    return Ok(SubmitResult::Skip);
                } else {
                    if err_str.contains(TRANSACTION_SCRIPT_ERROR)
                        || err_str.contains(TRANSACTION_EXCEEDED_MAXIMUM_BLOCK_BYTES_ERROR)
                    {
                        utils::dump_transaction(
                            &self.debug_config.debug_tx_dump_path,
                            &self.rpc_client,
                            &tx,
                        )
                        .await;
                    }

                    return Err(anyhow!(
                        "Fail to dry run transaction {}, error: {}",
                        hex::encode(tx.hash()),
                        err
                    ));
                }
            }
        };
        log::debug!(
            "[compose_next_block_submit_tx] dry run {}ms",
            t.elapsed().as_millis()
        );

        if cycles > self.debug_config.expected_l1_tx_upper_bound_cycles {
            log::warn!(
                "Submitting l2 block is cost unexpected cycles: {:?}, expected upper bound: {}",
                cycles,
                self.debug_config.expected_l1_tx_upper_bound_cycles
            );
            utils::dump_transaction(&self.debug_config.debug_tx_dump_path, &self.rpc_client, &tx)
                .await;
            return Err(anyhow!(
                "Submitting l2 block cycles exceeded limitation, cycles: {:?}",
                cycles
            ));
        }

        // send transaction
        match self.rpc_client.send_transaction(&tx).await {
            Ok(tx_hash) => {
                log::info!(
                    "Submitted l2 block {} in tx {}",
                    block_number,
                    hex::encode(tx_hash.as_slice())
                );
                Ok(SubmitResult::Submitted)
            }
            Err(err) => {
                log::error!("Submitting l2 block error: {}", err);

                // dumping script error transactions
                let err_str = err.to_string();
                if err_str.contains(TRANSACTION_SCRIPT_ERROR)
                    || err_str.contains(TRANSACTION_EXCEEDED_MAXIMUM_BLOCK_BYTES_ERROR)
                {
                    // dumping failed tx
                    utils::dump_transaction(
                        &self.debug_config.debug_tx_dump_path,
                        &self.rpc_client,
                        &tx,
                    )
                    .await;
                    Err(anyhow!("Submitting l2 block error: {}", err))
                } else {
                    // ignore non script error
                    let since_last_committed_secs = self
                        .last_committed_l2_block
                        .committed_at
                        .elapsed()
                        .as_secs();
                    if since_last_committed_secs < WAIT_PRODUCE_BLOCK_SECONDS {
                        log::debug!(
                            "last committed is {}s ago, dump tx",
                            since_last_committed_secs
                        );
                        // dumping failed tx
                        utils::dump_transaction(
                            &self.debug_config.debug_tx_dump_path,
                            &self.rpc_client,
                            &tx,
                        )
                        .await;
                    } else {
                        log::debug!("Skip dumping non-script-error tx");
                    }
                    Ok(SubmitResult::Skip)
                }
            }
        }
    }

    async fn complete_tx_skeleton(&self, args: CompleteTxArgs) -> Result<Transaction> {
        let CompleteTxArgs {
            deposit_cells,
            finalized_custodians,
            block,
            global_state,
            median_time,
            rollup_cell,
            withdrawal_extras,
        } = args;

        let rollup_context = self.generator.rollup_context();
        let mut tx_skeleton = TransactionSkeleton::default();
        // rollup cell
        tx_skeleton.inputs_mut().push(InputCellInfo {
            input: CellInput::new_builder()
                .previous_output(rollup_cell.out_point.clone())
                .since(input_since_from(median_time).pack())
                .build(),
            cell: rollup_cell.clone(),
        });
        let contracts_dep = self.contracts_dep_manager.load();
        // rollup deps
        tx_skeleton
            .cell_deps_mut()
            .push(contracts_dep.rollup_cell_type.clone().into());
        // rollup config cell
        tx_skeleton
            .cell_deps_mut()
            .push(self.config.rollup_config_cell_dep.clone().into());
        // deposit lock dep
        if !deposit_cells.is_empty() {
            let cell_dep: CellDep = contracts_dep.deposit_cell_lock.clone().into();
            tx_skeleton
                .cell_deps_mut()
                .push(CellDep::new_unchecked(cell_dep.as_bytes()));
        }
        // secp256k1 lock, used for unlock tx fee payment cells
        tx_skeleton
            .cell_deps_mut()
            .push(self.ckb_genesis_info.sighash_dep());

        // Package pending revert withdrawals and custodians
        let db = { self.chain.lock().await.store().begin_transaction() };
        let reverted_block_hashes = db.get_reverted_block_hashes()?;

        let rpc_client = &self.rpc_client;
        let (revert_custodians, mut collected_block_hashes) = rpc_client
            .query_custodian_cells_by_block_hashes(&reverted_block_hashes)
            .await?;
        let (revert_withdrawals, block_hashes) = rpc_client
            .query_withdrawal_cells_by_block_hashes(&reverted_block_hashes)
            .await?;
        collected_block_hashes.extend(block_hashes);

        // rollup action
        let rollup_action = {
            let submit_builder = if !collected_block_hashes.is_empty() {
                let db = self.store.begin_transaction();
                let block_smt = db.reverted_block_smt()?;

                let local_root: &H256 = block_smt.root();
                let global_revert_block_root: H256 = global_state.reverted_block_root().unpack();
                assert_eq!(local_root, &global_revert_block_root);

                let keys: Vec<H256> = collected_block_hashes.into_iter().collect();
                let leaves = keys.iter().map(|hash| (*hash, H256::one()));
                let proof = block_smt
                    .merkle_proof(keys.clone())?
                    .compile(leaves.collect())?;
                for key in keys.iter() {
                    log::info!("submit revert block {:?}", hex::encode(key.as_slice()));
                }

                RollupSubmitBlock::new_builder()
                    .reverted_block_hashes(keys.pack())
                    .reverted_block_proof(proof.0.pack())
            } else {
                RollupSubmitBlock::new_builder()
            };

            let submit_block = submit_builder.block(block.clone()).build();

            RollupAction::new_builder()
                .set(RollupActionUnion::RollupSubmitBlock(submit_block))
                .build()
        };

        // witnesses
        tx_skeleton.witnesses_mut().push(
            WitnessArgs::new_builder()
                .output_type(Some(rollup_action.as_bytes()).pack())
                .build(),
        );

        // output
        let output_data = global_state.as_bytes();
        let output = {
            let dummy = rollup_cell.output.clone();
            let capacity = dummy
                .occupied_capacity(output_data.len())
                .expect("capacity overflow");
            dummy.as_builder().capacity(capacity.pack()).build()
        };
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

        // Some deposit cells might have type scripts for sUDTs, handle cell deps
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

        // stake cell
        let generated_stake = crate::stake::generate(
            &rollup_cell,
            rollup_context,
            &block,
            &contracts_dep,
            &self.rpc_client,
            self.wallet.lock_script().to_owned(),
        )
        .await?;
        tx_skeleton.cell_deps_mut().extend(generated_stake.deps);
        tx_skeleton.inputs_mut().extend(generated_stake.inputs);
        tx_skeleton
            .outputs_mut()
            .push((generated_stake.output, generated_stake.output_data));

        // withdrawal cells
        let map_withdrawal_extras = withdrawal_extras.into_iter().map(|w| (w.hash().into(), w));
        if let Some(generated_withdrawal_cells) = crate::withdrawal::generate(
            rollup_context,
            finalized_custodians,
            &block,
            &contracts_dep,
            &map_withdrawal_extras.collect(),
        )? {
            tx_skeleton
                .cell_deps_mut()
                .extend(generated_withdrawal_cells.deps);
            tx_skeleton
                .inputs_mut()
                .extend(generated_withdrawal_cells.inputs);
            tx_skeleton
                .outputs_mut()
                .extend(generated_withdrawal_cells.outputs);
        }

        if let Some(reverted_deposits) =
            crate::deposit::revert(rollup_context, &contracts_dep, revert_custodians)?
        {
            log::info!("reverted deposits {}", reverted_deposits.inputs.len());

            tx_skeleton.cell_deps_mut().extend(reverted_deposits.deps);

            let input_len = tx_skeleton.inputs().len();
            let witness_len = tx_skeleton.witnesses_mut().len();
            if input_len != witness_len {
                // append dummy witness args to align our reverted deposit witness args
                let dummy_witness_argses = (0..input_len - witness_len)
                    .into_iter()
                    .map(|_| WitnessArgs::default())
                    .collect::<Vec<_>>();
                tx_skeleton.witnesses_mut().extend(dummy_witness_argses);
            }

            tx_skeleton.inputs_mut().extend(reverted_deposits.inputs);
            tx_skeleton
                .witnesses_mut()
                .extend(reverted_deposits.witness_args);
            tx_skeleton.outputs_mut().extend(reverted_deposits.outputs);
        }

        // reverted withdrawal cells
        if let Some(reverted_withdrawals) =
            crate::withdrawal::revert(rollup_context, &contracts_dep, revert_withdrawals)?
        {
            log::info!("reverted withdrawals {}", reverted_withdrawals.inputs.len());

            tx_skeleton
                .cell_deps_mut()
                .extend(reverted_withdrawals.deps);

            let input_len = tx_skeleton.inputs().len();
            let witness_len = tx_skeleton.witnesses_mut().len();
            if input_len != witness_len {
                // append dummy witness args to align our reverted withdrawal witness args
                let dummy_witness_argses = (0..input_len - witness_len)
                    .into_iter()
                    .map(|_| WitnessArgs::default())
                    .collect::<Vec<_>>();
                tx_skeleton.witnesses_mut().extend(dummy_witness_argses);
            }

            tx_skeleton.inputs_mut().extend(reverted_withdrawals.inputs);
            tx_skeleton
                .witnesses_mut()
                .extend(reverted_withdrawals.witness_args);
            tx_skeleton
                .outputs_mut()
                .extend(reverted_withdrawals.outputs);
        }

        // check cell dep duplicate (deposits, withdrawals, reverted_withdrawals sudt type dep)
        {
            let deps: HashSet<_> = tx_skeleton.cell_deps_mut().iter().collect();
            *tx_skeleton.cell_deps_mut() = deps.into_iter().cloned().collect();
        }

        // tx fee cell
        fill_tx_fee(
            &mut tx_skeleton,
            &self.rpc_client.indexer,
            self.wallet.lock_script().to_owned(),
        )
        .await?;
        debug_assert_eq!(
            tx_skeleton.taken_outpoints()?.len(),
            tx_skeleton.inputs().len(),
            "check duplicated inputs"
        );
        // sign
        let tx = self.wallet.sign_tx_skeleton(tx_skeleton)?;
        log::debug!("final tx size: {}", tx.as_slice().len());
        Ok(tx)
    }
}

fn input_since_from(median_time: Duration) -> u64 {
    /// Transaction since flag
    const SINCE_BLOCK_TIMESTAMP_FLAG: u64 = 0x4000_0000_0000_0000;
    SINCE_BLOCK_TIMESTAMP_FLAG | median_time.as_secs()
}

struct CompleteTxArgs {
    deposit_cells: Vec<DepositInfo>,
    finalized_custodians: CollectedCustodianCells,
    block: L2Block,
    global_state: GlobalState,
    median_time: Duration,
    rollup_cell: CellInfo,
    withdrawal_extras: Vec<WithdrawalRequestExtra>,
}
