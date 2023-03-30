#![allow(clippy::mutable_key_type)]
//! Block producing and block submit tx composing.

use std::{collections::HashSet, sync::Arc, time::Instant};

use anyhow::{bail, ensure, Context, Result};
use ckb_chain_spec::consensus::MAX_BLOCK_BYTES;
use gw_chain::chain::Chain;
use gw_config::{BlockProducerConfig, ContractsCellDep};
use gw_generator::Generator;
use gw_jsonrpc_types::{test_mode::TestModePayload, JsonCalcHash};
use gw_mem_pool::{
    custodian::to_custodian_cell,
    pool::{MemPool, OutputParam},
};
use gw_rpc_client::{contract::ContractsCellDepManager, rpc_client::RPCClient};
use gw_smt::smt::SMTH256;
use gw_store::Store;
use gw_types::{
    bytes::Bytes,
    core::ScriptHashType,
    h256::*,
    offchain::{global_state_from_slice, CompatibleFinalizedTimepoint, DepositInfo, InputCellInfo},
    packed::{
        CellDep, CellOutput, GlobalState, L2Block, RollupAction, RollupActionUnion,
        RollupSubmitBlock, Script, Transaction, WithdrawalRequestExtra, WitnessArgs,
    },
    prelude::*,
};
use gw_utils::{
    fee::fill_tx_fee_with_local, finalized_timepoint, genesis_info::CKBGenesisInfo,
    local_cells::LocalCellsManager, query_rollup_cell, since::Since,
    transaction_skeleton::TransactionSkeleton, wallet::Wallet, RollupContext,
};
use tokio::sync::Mutex;
use tracing::instrument;

use crate::{
    custodian::query_mergeable_custodians,
    produce_block::{
        generate_produce_block_param, produce_block, ProduceBlockParam, ProduceBlockResult,
    },
    test_mode_control::TestModeControl,
};

/// 524_288 we choose this value because it is smaller than the MAX_BLOCK_BYTES which is 597K
const MAX_ROLLUP_WITNESS_SIZE: usize = 1 << 19;
/// How many extra size are needed for the rollup WitnessArgs compared to the
/// L2Block if there are no reverted blocks.
const ROLLUP_WITNESS_OVERHEAD: usize = 48;

pub fn check_block_size(block_size: usize) -> Result<()> {
    if block_size >= MAX_ROLLUP_WITNESS_SIZE - ROLLUP_WITNESS_OVERHEAD {
        bail!(TransactionSizeError::WitnessTooLarge)
    }
    Ok(())
}

fn generate_custodian_cells(
    rollup_context: &RollupContext,
    block: &L2Block,
    deposit_cells: &[DepositInfo],
) -> Vec<(CellOutput, Bytes)> {
    let block_hash: H256 = block.hash();
    let finalized_timepoint = finalized_timepoint(
        &rollup_context.rollup_config,
        &rollup_context.fork_config,
        block.raw().number().unpack(),
        block.raw().timestamp().unpack(),
    );
    let to_custodian = |deposit| -> _ {
        to_custodian_cell(rollup_context, &block_hash, &finalized_timepoint, deposit)
            .expect("sanitized deposit")
    };

    deposit_cells.iter().map(to_custodian).collect()
}

pub struct BlockProducer {
    rollup_config_hash: H256,
    store: Store,
    chain: Arc<Mutex<Chain>>,
    generator: Arc<Generator>,
    wallet: Wallet,
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
            rpc_client,
            ckb_genesis_info,
            config,
            tests_control,
            contracts_dep_manager,
        } = args;

        let wallet = match config.wallet_config {
            Some(ref c) => Wallet::from_config(c).with_context(|| "init wallet")?,
            None => bail!("no wallet config for block producer"),
        };

        let block_producer = BlockProducer {
            rollup_config_hash,
            generator,
            chain,
            rpc_client,
            wallet,
            ckb_genesis_info,
            tests_control,
            store,
            contracts_dep_manager,
        };
        Ok(block_producer)
    }

    pub fn generator(&self) -> &Generator {
        &self.generator
    }

    pub fn contracts_dep_manager(&self) -> &ContractsCellDepManager {
        &self.contracts_dep_manager
    }

    #[instrument(skip_all, fields(retry_count = retry_count))]
    pub async fn produce_next_block(
        &self,
        mem_pool: &mut MemPool,
        retry_count: usize,
    ) -> Result<ProduceBlockResult> {
        if let Some(ref tests_control) = self.tests_control {
            match tests_control.payload().await {
                Some(TestModePayload::None) => tests_control.clear_none().await?,
                Some(TestModePayload::BadBlock { .. }) => (),
                _ => unreachable!(),
            }
        }

        // get txs & withdrawal requests from mem pool
        let (mut mem_block, post_block_state) = {
            let t = Instant::now();
            let r = mem_pool.output_mem_block(&OutputParam::new(retry_count));
            log::debug!(
                target: "produce-block", "output mem block {}ms",
                t.elapsed().as_millis()
            );
            r
        };

        let remaining_capacity = mem_block.take_finalized_custodians_capacity();
        let t = Instant::now();
        let block_param = generate_produce_block_param(&self.store, mem_block, post_block_state)?;

        log::debug!(
            target: "produce-block",
            "generate produce block param {}ms",
            t.elapsed().as_millis()
        );

        // produce block
        let reverted_block_root: H256 = {
            let mut db = self.store.begin_transaction();
            let smt = db.reverted_block_smt()?;
            (*smt.root()).into()
        };

        let param = ProduceBlockParam {
            stake_cell_owner_lock_hash: self.wallet.lock_script().hash(),
            reverted_block_root,
            rollup_config_hash: self.rollup_config_hash,
            block_param,
        };
        let mut db = self.store.begin_transaction();
        let mut result = produce_block(&mut db, &self.generator, param)?;
        result.remaining_capacity = remaining_capacity;
        Ok(result)
    }

    #[instrument(skip_all, fields(block = args.block.raw().number().unpack()))]
    pub async fn compose_submit_tx(&self, args: ComposeSubmitTxArgs<'_>) -> Result<Transaction> {
        let ComposeSubmitTxArgs {
            deposit_cells,
            block,
            global_state,
            since,
            withdrawal_extras,
            local_cells_manager,
            fee_rate,
        } = args;

        let rollup_cell = query_rollup_cell(local_cells_manager, &self.rpc_client)
            .await?
            .context("rollup cell not found")?;

        let rollup_context = self.generator.rollup_context();
        let contracts_dep = self.contracts_dep_manager.load();
        let omni_lock_code_hash = self.contracts_dep_manager.load_scripts().omni_lock.hash();
        let mut tx_skeleton = TransactionSkeleton::new(omni_lock_code_hash.0);

        let deps = [
            &contracts_dep.rollup_cell_type,
            rollup_context.rollup_config_cell_dep(),
            // TODO: remove after migrating to delegate-cell-lock.
            &contracts_dep.omni_lock,
        ];
        let opt_deps = [
            contracts_dep.delegate_cell.as_ref(),
            contracts_dep.delegate_cell_lock.as_ref(),
        ];
        let rollup_deps = deps
            .into_iter()
            .chain(opt_deps.into_iter().flatten())
            .map(|d| d.clone().into());

        // rollup cell
        tx_skeleton.inputs_mut().push(InputCellInfo::with_since(
            rollup_cell.clone(),
            since.as_u64(),
        ));
        // rollup deps
        tx_skeleton.cell_deps_mut().extend(rollup_deps);
        // deposit lock dep
        if !deposit_cells.is_empty() {
            let cell_dep: CellDep = contracts_dep.deposit_cell_lock.clone().into();
            tx_skeleton.cell_deps_mut().push(cell_dep);
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
                let mut db = self.store.begin_transaction();
                let block_smt = db.reverted_block_smt()?;

                let local_root: H256 = (*block_smt.root()).into();
                let global_revert_block_root: H256 = global_state.reverted_block_root().unpack();
                assert_eq!(local_root, global_revert_block_root);

                let keys: Vec<H256> = collected_block_hashes.into_iter().collect();
                for key in keys.iter() {
                    log::info!("submit revert block {:?}", hex::encode(key.as_slice()));
                }
                let reverted_block_hashes = keys.pack();
                let proof = {
                    let smt_keys: Vec<SMTH256> = keys.into_iter().map(Into::into).collect();
                    block_smt
                        .merkle_proof(smt_keys.clone())?
                        .compile(smt_keys)?
                };

                RollupSubmitBlock::new_builder()
                    .reverted_block_hashes(reverted_block_hashes)
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
        let witness = WitnessArgs::new_builder()
            .output_type(Some(rollup_action.as_bytes()).pack())
            .build();
        ensure!(
            witness.as_slice().len() < MAX_ROLLUP_WITNESS_SIZE,
            TransactionSizeError::WitnessTooLarge,
        );
        tx_skeleton.witnesses_mut().push(witness);

        // output

        // Try to change rollup cell lock to delegate cell lock.
        //
        // TODO: remove this when lock has been upgraded on all networks.
        let scripts = self.contracts_dep_manager.load_scripts();
        let d = scripts.delegate_cell.as_ref().map(|s| s.hash());
        let dl = scripts.delegate_cell_lock.as_ref().map(|s| s.hash());
        let rollup_output = if let (Some(d), Some(dl)) = (d, dl) {
            let new_lock = Script::new_builder()
                .code_hash(dl.pack())
                .hash_type(ScriptHashType::Type.into())
                .args(d.as_bytes().pack())
                .build();
            let old_lock = rollup_cell.output.as_reader().lock();
            if old_lock.as_slice() != new_lock.as_slice() {
                if let Err(e) = self.check_delegate_cell_lock(&contracts_dep).await {
                    log::warn!(
                        "check delegate cell lock failed, not changing lock: {:#}",
                        e
                    );
                    rollup_cell.output.clone()
                } else {
                    log::info!("chaging lock from {:?} to {:?}", old_lock, new_lock);
                    rollup_cell
                        .output
                        .clone()
                        .as_builder()
                        .lock(new_lock)
                        .build()
                }
            } else {
                rollup_cell.output.clone()
            }
        } else {
            rollup_cell.output.clone()
        };

        let output_data = global_state.as_bytes();
        let output = {
            let capacity = rollup_output
                .occupied_capacity_bytes(output_data.len())
                .expect("capacity overflow");
            rollup_output.as_builder().capacity(capacity.pack()).build()
        };
        tx_skeleton.outputs_mut().push((output, output_data));

        // deposit cells
        for deposit in &deposit_cells {
            log::info!("using deposit cell {:?}", deposit.cell.out_point);
            tx_skeleton.inputs_mut().push(deposit.cell.clone().into());
        }

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
            local_cells_manager,
        )
        .await?;
        tx_skeleton.cell_deps_mut().extend(generated_stake.deps);
        tx_skeleton.inputs_mut().extend(generated_stake.inputs);
        tx_skeleton
            .outputs_mut()
            .push((generated_stake.output, generated_stake.output_data));

        let prev_global_state = global_state_from_slice(&rollup_cell.data)?;
        let prev_compatible_finalized_timepoint = CompatibleFinalizedTimepoint::from_global_state(
            &prev_global_state,
            rollup_context.rollup_config.finality_blocks().unpack(),
        );
        let finalized_custodians = gw_mem_pool::custodian::query_finalized_custodians(
            rpc_client,
            &self.store.get_snapshot(),
            withdrawal_extras.iter().map(|w| w.request()),
            rollup_context,
            &prev_compatible_finalized_timepoint,
            local_cells_manager,
        )
        .await?
        .expect_any();
        let finalized_custodians = query_mergeable_custodians(
            local_cells_manager,
            rpc_client,
            finalized_custodians,
            &prev_compatible_finalized_timepoint,
        )
        .await?
        .expect_any();

        // Simple UDT dep
        if !deposit_cells.is_empty()
            || !withdrawal_extras.is_empty()
            || !finalized_custodians.sudt.is_empty()
        {
            tx_skeleton
                .cell_deps_mut()
                .push(contracts_dep.l1_sudt_type.clone().into());
        }

        // withdrawal cells
        let map_withdrawal_extras = withdrawal_extras.into_iter().map(|w| (w.hash(), w));
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
            local_cells_manager,
            fee_rate,
        )
        .await?;
        debug_assert_eq!(
            tx_skeleton.taken_outpoints()?.len(),
            tx_skeleton.inputs().len(),
            "check duplicated inputs"
        );
        // sign
        let tx = self.wallet.sign_tx_skeleton(tx_skeleton)?;
        ensure!(
            (tx.as_slice().len() as u64) < MAX_BLOCK_BYTES,
            TransactionSizeError::TransactionTooLarge
        );
        log::debug!("final tx size: {}", tx.as_slice().len());
        Ok(tx)
    }

    // TODO: remove after migrating to delegate cell.
    /// Check delegate cell lock and delegate cell.
    async fn check_delegate_cell_lock(&self, contracts_dep: &ContractsCellDep) -> Result<()> {
        let delegate_cell_lock_dep = contracts_dep.delegate_cell_lock.as_ref().unwrap();
        let delegate_cell_lock_cell = self
            .rpc_client
            .get_cell(delegate_cell_lock_dep.out_point.clone().into())
            .await?
            .context("get delegate cell lock cell")?;
        let delegate_cell_lock_data = delegate_cell_lock_cell.cell.unwrap().data;
        // This is short living code, so just hard code the script path.
        let delegate_cell_lock_program =
            std::fs::read("/scripts/godwoken-scripts/delegate-cell-lock")
                .context("load delegate cell lock program")?;
        if delegate_cell_lock_data != delegate_cell_lock_program {
            bail!("delegate cell lock program mismatch");
        }

        let delegate_cell = contracts_dep.delegate_cell.as_ref().unwrap();
        let delegate_cell = self
            .rpc_client
            .get_cell(delegate_cell.out_point.clone().into())
            .await?
            .context("get delegate cell")?;
        let delegate_cell_data = delegate_cell.cell.unwrap().data;
        let wallet_lock_hash = self.wallet.lock_script().hash();
        ensure!(
            delegate_cell_data == wallet_lock_hash[..20],
            "delegate cell data does not match wallet lock hash 160"
        );

        Ok(())
    }
}

pub struct ComposeSubmitTxArgs<'a> {
    pub deposit_cells: Vec<DepositInfo>,
    pub block: L2Block,
    pub global_state: GlobalState,
    pub since: Since,
    pub withdrawal_extras: Vec<WithdrawalRequestExtra>,
    pub local_cells_manager: &'a LocalCellsManager,
    pub fee_rate: u64,
}

#[derive(thiserror::Error, Debug)]
pub enum TransactionSizeError {
    #[error("transaction too large")]
    TransactionTooLarge,
    #[error("witness too large")]
    WitnessTooLarge,
}

#[test]
fn test_witness_size_overhead() {
    let block = L2Block::default();
    let submit = RollupSubmitBlock::new_builder()
        .block(block.clone())
        .build();
    let action = RollupAction::new_builder().set(submit).build();
    let witness = WitnessArgs::new_builder()
        .output_type(Some(action.as_bytes()).pack())
        .build();
    assert_eq!(
        witness.as_slice().len() - block.as_slice().len(),
        ROLLUP_WITNESS_OVERHEAD
    );
}
