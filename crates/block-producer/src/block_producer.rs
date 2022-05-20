#![allow(clippy::mutable_key_type)]
//! Block producing and block submit tx composing.

use crate::{
    custodian::query_mergeable_custodians,
    produce_block::{
        generate_produce_block_param, produce_block, ProduceBlockParam, ProduceBlockResult,
    },
    test_mode_control::TestModeControl,
};

use anyhow::{Context, Result};

use gw_chain::chain::Chain;
use gw_common::{h256_ext::H256Ext, H256};
use gw_config::BlockProducerConfig;
use gw_generator::Generator;
use gw_jsonrpc_types::test_mode::TestModePayload;
use gw_mem_pool::{
    custodian::to_custodian_cell,
    pool::{MemPool, OutputParam},
};
use gw_rpc_client::{contract::ContractsCellDepManager, rpc_client::RPCClient};
use gw_store::Store;
use gw_types::{
    bytes::Bytes,
    offchain::{CellInfo, CollectedCustodianCells, DepositInfo, InputCellInfo, RollupContext},
    packed::{
        CellDep, CellInput, CellOutput, GlobalState, L2Block, OutPoint, RollupAction,
        RollupActionUnion, RollupSubmitBlock, Transaction, WithdrawalRequestExtra, WitnessArgs,
    },
    prelude::*,
};
use gw_utils::{
    fee::fill_tx_fee_with_local, genesis_info::CKBGenesisInfo, since::Since,
    transaction_skeleton::TransactionSkeleton, wallet::Wallet,
};
use std::{collections::HashSet, sync::Arc, time::Instant};
use tokio::sync::Mutex;
use tracing::instrument;

const MAX_BLOCK_OUTPUT_PARAM_RETRY_COUNT: usize = 10;
const TRANSACTION_SCRIPT_ERROR: &str = "TransactionScriptError";
const TRANSACTION_EXCEEDED_MAXIMUM_BLOCK_BYTES_ERROR: &str = "ExceededMaximumBlockBytes";
const TRANSACTION_FAILED_TO_RESOLVE_ERROR: &str = "TransactionFailedToResolve";
/// 524_288 we choose this value because it is smaller than the MAX_BLOCK_BYTES which is 597K
const MAX_ROLLUP_WITNESS_SIZE: usize = 1 << 19;

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

pub struct BlockProducer {
    rollup_config_hash: H256,
    store: Store,
    chain: Arc<Mutex<Chain>>,
    mem_pool: Arc<Mutex<MemPool>>,
    generator: Arc<Generator>,
    wallet: Wallet,
    config: BlockProducerConfig,
    rpc_client: RPCClient,
    ckb_genesis_info: CKBGenesisInfo,
    tests_control: Option<TestModeControl>,
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
            tests_control,
            store,
            contracts_dep_manager,
        };
        Ok(block_producer)
    }

    pub fn last_submitted_tx_hash(&self) -> Arc<tokio::sync::RwLock<H256>> {
        self.last_submitted_tx_hash.clone()
    }

    #[instrument(skip_all, fields(event = %event))]
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

        let rollup_input_since = match self.rpc_client.get_block_median_time(tip_hash).await? {
            Some(median_time) => {
                let tip_block_timestamp = Duration::from_millis(
                    self.store
                        .get_last_valid_tip_block()?
                        .raw()
                        .timestamp()
                        .unpack(),
                );
                if median_time < tip_block_timestamp {
                    log::warn!("[block producer] median time is less than tip block timestamp, skip produce new block");
                    return Ok(());
                }
                InputSince::from_median_time(median_time)
            }
            None => return Ok(()),
        };

        // try issue next block
        let mut retry_count = 0;
        while retry_count <= MAX_BLOCK_OUTPUT_PARAM_RETRY_COUNT {
            let (block_number, tx, next_global_state) = match self
                .compose_next_block_submit_tx(rollup_input_since, rollup_cell.clone(), retry_count)
                .await
            {
                Ok((block_number, tx, next_global_state)) => (block_number, tx, next_global_state),
                Err(err) if err.downcast_ref::<GreaterBlockTimestampError>().is_some() => {
                    // Wait next l1 tip block median time
                    log::debug!(
                        "[produce block] block timestamp is greater than rollup input since, wait next median time"
                    );
                    return Ok(());
                }
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
            if global_state.rollup_config_hash() != next_global_state.rollup_config_hash() {
                bail!("different rollup config hash, please check config.toml");
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

    #[instrument(skip_all, fields(retry_count = retry_count))]
    pub async fn produce_next_block(&self, retry_count: usize) -> Result<ProduceBlockResult> {
        if let Some(ref tests_control) = self.tests_control {
            match tests_control.payload().await {
                Some(TestModePayload::None) => tests_control.clear_none().await?,
                Some(TestModePayload::BadBlock { .. }) => (),
                _ => unreachable!(),
            }
        }

        // get txs & withdrawal requests from mem pool
        let (finalized_custodians, block_param) = {
            let (mem_block, post_block_state) = {
                let t = Instant::now();
                log::debug!(target: "produce-block", "acquire mem-pool",);
                let mem_pool = self.mem_pool.lock().await;
                log::debug!(
                    target: "produce-block", "unlock mem-pool {}ms",
                    t.elapsed().as_millis()
                );
                let t = Instant::now();
                let r = mem_pool.output_mem_block(&OutputParam::new(retry_count));
                log::debug!(
                    target: "produce-block", "output mem block {}ms",
                    t.elapsed().as_millis()
                );
                r
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
                target: "produce-block",
                "finalized custodians {:?}",
                finalized_custodians.cells_info.len()
            );

            log::debug!(
                target: "produce-block",
                "generate produce block param {}ms",
                t.elapsed().as_millis()
            );
            (finalized_custodians, produce_block_param)
        };

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
            finalized_custodians,
        };
        let db = self.store.begin_transaction();
        let block_result = produce_block(&db, &self.generator, param)?;
        let ProduceBlockResult {
            mut block,
            mut global_state,
            withdrawal_extras,
            deposit_cells,
            finalized_custodians,
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
            Ok((number, tx, global_state))
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

    #[instrument(skip_all, fields(block = args.block.raw().number().unpack()))]
    pub async fn compose_submit_tx(
        &self,
        args: ComposeSubmitTxArgs<'_>,
    ) -> Result<(Transaction, CellInfo)> {
        let ComposeSubmitTxArgs {
            deposit_cells,
            finalized_custodians,
            block,
            global_state,
            since,
            rollup_cell,
            withdrawal_extras,
            local_spent_payment_cells,
            local_live_payment_cells,
        } = args;

        let rollup_context = self.generator.rollup_context();
        let omni_lock_code_hash = self.contracts_dep_manager.load_scripts().omni_lock.hash();
        let mut tx_skeleton = TransactionSkeleton::new(omni_lock_code_hash.0);

        // rollup cell
        tx_skeleton.inputs_mut().push(InputCellInfo {
            input: CellInput::new_builder()
                .previous_output(rollup_cell.out_point.clone())
                .since(since.as_u64().pack())
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
        // omni lock
        tx_skeleton
            .cell_deps_mut()
            .push(contracts_dep.omni_lock.clone().into());

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
        let rollup_cell_output = output.clone();
        let rollup_cell_output_data = output_data.clone();
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

        // Simple UDT dep
        tx_skeleton
            .cell_deps_mut()
            .push(contracts_dep.l1_sudt_type.clone().into());

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
        fill_tx_fee_with_local(
            &mut tx_skeleton,
            &self.rpc_client.indexer,
            self.wallet.lock_script().to_owned(),
            local_spent_payment_cells,
            local_live_payment_cells,
        )
        .await?;
        debug_assert_eq!(
            tx_skeleton.taken_outpoints()?.len(),
            tx_skeleton.inputs().len(),
            "check duplicated inputs"
        );
        // sign
        let tx = self.wallet.sign_tx_skeleton(tx_skeleton)?;
        // TODO: check tx size witnesses size.
        let tx_hash = tx.hash();
        log::debug!("final tx size: {}", tx.as_slice().len());
        Ok((
            tx,
            CellInfo {
                output: rollup_cell_output,
                data: rollup_cell_output_data,
                out_point: OutPoint::new_builder()
                    .tx_hash(tx_hash.pack())
                    // Rollup cell is the first output.
                    .index(0u32.pack())
                    .build(),
            },
        ))
    }
}

pub struct ComposeSubmitTxArgs<'a> {
    pub deposit_cells: Vec<DepositInfo>,
    pub finalized_custodians: CollectedCustodianCells,
    pub block: L2Block,
    pub global_state: GlobalState,
    pub since: Since,
    pub rollup_cell: CellInfo,
    pub withdrawal_extras: Vec<WithdrawalRequestExtra>,
    pub local_spent_payment_cells: &'a HashSet<OutPoint>,
    pub local_live_payment_cells: &'a [CellInfo],
}
