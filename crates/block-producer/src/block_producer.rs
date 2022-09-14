#![allow(clippy::mutable_key_type)]
//! Block producing and block submit tx composing.

use crate::{
    custodian::query_mergeable_custodians,
    produce_block::{
        generate_produce_block_param, produce_block, ProduceBlockParam, ProduceBlockResult,
    },
    test_mode_control::TestModeControl,
};

use anyhow::{bail, ensure, Context, Result};
use ckb_chain_spec::consensus::MAX_BLOCK_BYTES;
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
    offchain::{DepositInfo, InputCellInfo, RollupContext},
    packed::{
        CellDep, CellInput, CellOutput, GlobalState, L2Block, RollupAction, RollupActionUnion,
        RollupSubmitBlock, Transaction, WithdrawalRequestExtra, WitnessArgs,
    },
    prelude::*,
};
use gw_utils::{
    fee::fill_tx_fee_with_local, genesis_info::CKBGenesisInfo, local_cells::LocalCellsManager,
    query_rollup_cell, since::Since, transaction_skeleton::TransactionSkeleton, wallet::Wallet,
};
use std::{collections::HashSet, sync::Arc, time::Instant};
use tokio::sync::Mutex;
use tracing::instrument;

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
            config,
            tests_control,
            store,
            contracts_dep_manager,
        };
        Ok(block_producer)
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
        let mut result = produce_block(&db, &self.generator, param)?;
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
        } = args;

        let rollup_cell = query_rollup_cell(local_cells_manager, &self.rpc_client)
            .await?
            .context("rollup cell not found")?;

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
        let witness = WitnessArgs::new_builder()
            .output_type(Some(rollup_action.as_bytes()).pack())
            .build();
        ensure!(
            witness.as_slice().len() < MAX_ROLLUP_WITNESS_SIZE,
            TransactionSizeError::WitnessTooLarge,
        );
        tx_skeleton.witnesses_mut().push(witness);

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
            log::info!("using deposit cell {:?}", deposit.cell.out_point);
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
            local_cells_manager,
        )
        .await?;
        tx_skeleton.cell_deps_mut().extend(generated_stake.deps);
        tx_skeleton.inputs_mut().extend(generated_stake.inputs);
        tx_skeleton
            .outputs_mut()
            .push((generated_stake.output, generated_stake.output_data));

        let last_finalized_block_number = self
            .generator
            .rollup_context()
            .last_finalized_block_number(block.raw().number().unpack() - 1);
        let finalized_custodians = gw_mem_pool::custodian::query_finalized_custodians(
            rpc_client,
            &self.store.get_snapshot(),
            withdrawal_extras.iter().map(|w| w.request()),
            rollup_context,
            last_finalized_block_number,
            local_cells_manager,
        )
        .await?
        .expect_any();
        let finalized_custodians = query_mergeable_custodians(
            local_cells_manager,
            rpc_client,
            finalized_custodians,
            last_finalized_block_number,
        )
        .await?
        .expect_any();

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
            local_cells_manager,
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
}

pub struct ComposeSubmitTxArgs<'a> {
    pub deposit_cells: Vec<DepositInfo>,
    pub block: L2Block,
    pub global_state: GlobalState,
    pub since: Since,
    pub withdrawal_extras: Vec<WithdrawalRequestExtra>,
    pub local_cells_manager: &'a LocalCellsManager,
}

#[derive(thiserror::Error, Debug)]
pub enum TransactionSizeError {
    #[error("transaction too large")]
    TransactionTooLarge,
    #[error("witness too large")]
    WitnessTooLarge,
}
