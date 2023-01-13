#![allow(clippy::mutable_key_type)]

use anyhow::{anyhow, bail, Result};
use ckb_chain_spec::consensus::MAX_BLOCK_BYTES;
use gw_common::registry_address::RegistryAddress;
use gw_config::{BlockProducerConfig, DebugConfig, OffChainValidatorConfig};
use gw_rpc_client::contract::ContractsCellDepManager;
use gw_rpc_client::rpc_client::RPCClient;
use gw_store::state::MemStateDB;
use gw_store::transaction::StoreTransaction;
use gw_types::core::DepType;
use gw_types::h256::*;
use gw_types::offchain::{CellInfo, InputCellInfo};
use gw_types::packed::{
    CellDep, CellInput, L2Block, OutPoint, OutPointVec, WithdrawalRequestExtra,
};
use gw_types::prelude::*;
use gw_utils::wallet::Wallet;
use gw_utils::RollupContext;

use std::{
    collections::{HashMap, HashSet},
    fs::{create_dir_all, write},
    path::PathBuf,
    sync::Arc,
};

pub mod mock_block;
pub mod mock_tx;
pub mod verify_tx;
pub use mock_tx::mock_cancel_challenge_tx;
pub use verify_tx::dump_tx;

use self::{
    mock_block::MockBlockParam,
    mock_tx::{MockOutput, MockRollup},
    verify_tx::{verify_tx, RollupCellDeps, TxWithContext},
};

// TODO: More properly value
const MAX_TX_WITHDRAWAL_PROOF_SIZE: u64 = 100 * 1024;
// TODO: Relax limit
const MARGIN_OF_MOCK_BLOCK_SAFITY_CYCLES: u64 = 5_000_000;
const MARGIN_OF_MOCK_BLOCK_SAFITY_TX_SIZE_LIMIT: u64 =
    MAX_BLOCK_BYTES - MAX_TX_WITHDRAWAL_PROOF_SIZE;

type MemTree = MemStateDB;

#[derive(Debug, Clone)]
pub struct CKBGenesisInfo {
    pub sighash_dep: CellDep,
}

#[derive(Clone)]
pub struct OffChainMockContext {
    pub contracts_dep_manager: ContractsCellDepManager,
    pub rollup_cell_deps: RollupCellDeps,
    pub mock_rollup: Arc<MockRollup>,
}

pub struct OffChainMockContextBuildArgs<'a> {
    pub rpc_client: &'a RPCClient,
    pub rollup_context: RollupContext,
    pub wallet: Wallet,
    pub config: BlockProducerConfig,
    pub ckb_genesis_info: CKBGenesisInfo,
    pub builtin_load_data: HashMap<H256, CellDep>,
    pub contracts_dep_manager: ContractsCellDepManager,
}

impl OffChainMockContext {
    pub async fn build(args: OffChainMockContextBuildArgs<'_>) -> Result<Self> {
        let OffChainMockContextBuildArgs {
            rpc_client,
            rollup_context,
            wallet,
            config,
            ckb_genesis_info,
            builtin_load_data,
            contracts_dep_manager,
        } = args;

        let rollup_cell = {
            let query = rpc_client.query_rollup_cell().await?;
            into_input_cell_info(query.ok_or_else(|| anyhow!("can't found rollup cell"))?)
        };

        let median_time = {
            let tip_block_hash = rpc_client.get_tip().await?.block_hash().unpack();
            let opt_time = rpc_client.get_block_median_time(tip_block_hash).await?;
            opt_time.ok_or_else(|| anyhow!("[offchain] tip block median time not found"))?
        };
        let rollup_type_script = rollup_cell.cell.output.type_();
        let mock_rollup = {
            let mock = MockRollup {
                rollup_type_script,
                rollup_context,
                median_time,
                wallet,
                config,
                ckb_genesis_info,
                builtin_load_data,
                contracts_dep_manager: contracts_dep_manager.clone(),
            };
            Arc::new(mock)
        };

        // FIXME: Update rollup_cell_deps after contracts dep manager refresh
        // FIXME: cancel challenge
        let rollup_deps: Vec<CellDep> = {
            let mut deps = vec![
                mock_rollup.cell_deps().rollup_cell_type.clone().into(),
                mock_rollup
                    .rollup_context
                    .fork_config
                    .chain
                    .rollup_config_cell_dep
                    .clone()
                    .into(),
                mock_rollup.cell_deps().challenge_cell_lock.clone().into(),
                mock_rollup.ckb_genesis_info.sighash_dep.clone(),
            ];
            deps.extend({
                let contract_deps = mock_rollup.cell_deps().allowed_contract_types.clone();
                contract_deps.into_values().map(CellDep::from)
            });
            deps.extend({
                let eoa_deps = mock_rollup.cell_deps().allowed_eoa_locks.clone();
                eoa_deps.into_values().map(CellDep::from)
            });
            deps.extend(mock_rollup.builtin_load_data.values().cloned());

            deps
        };
        let resolved_rollup_deps = resolve_cell_deps(rpc_client, rollup_deps).await?;
        let rollup_cell_deps = RollupCellDeps::new(resolved_rollup_deps);

        let mock_context = OffChainMockContext {
            contracts_dep_manager,
            rollup_cell_deps,
            mock_rollup,
        };

        Ok(mock_context)
    }

    pub fn replace_scripts(&self, scripts: &HashMap<ckb_types::H256, PathBuf>) -> Result<Self> {
        let mock_context = OffChainMockContext {
            contracts_dep_manager: self.contracts_dep_manager.clone(),
            rollup_cell_deps: self.rollup_cell_deps.replace_scripts(scripts)?,
            mock_rollup: Arc::clone(&self.mock_rollup),
        };

        Ok(mock_context)
    }
}

#[derive(Clone)]
pub struct OffChainValidatorContext {
    pub validator_config: Arc<OffChainValidatorConfig>,
    pub debug_config: Arc<DebugConfig>,
    pub rollup_cell_deps: RollupCellDeps,
    pub mock_rollup: Arc<MockRollup>,
}

impl OffChainValidatorContext {
    pub fn build(
        mock_context: &OffChainMockContext,
        debug_config: DebugConfig,
        validator_config: OffChainValidatorConfig,
    ) -> Result<Self> {
        if validator_config.verify_max_cycles <= MARGIN_OF_MOCK_BLOCK_SAFITY_CYCLES {
            bail!(
                "invalid verify max cycles, should be bigger than {}",
                MARGIN_OF_MOCK_BLOCK_SAFITY_CYCLES
            );
        }

        let rollup_cell_deps = mock_context.rollup_cell_deps.clone();
        let mock_rollup = mock_context.mock_rollup.clone();

        let debug_config = Arc::new(debug_config);
        let validator_config = Arc::new(validator_config);

        Ok(OffChainValidatorContext {
            validator_config,
            debug_config,
            rollup_cell_deps,
            mock_rollup,
        })
    }
}

#[derive(Debug)]
pub struct VerifyTxCycles {
    pub signature: Option<u64>,
    pub execution_by_witness: Option<u64>,
    pub execution_by_celldep: Option<u64>,
}

pub struct OffChainCancelChallengeValidator {
    validator_context: OffChainValidatorContext,
    safe_margin: MarginOfMockBlockSafity,
    block_param: MockBlockParam,
}

impl OffChainCancelChallengeValidator {
    pub fn new(
        ctx: OffChainValidatorContext,
        block_producer: RegistryAddress,
        parent_block: &L2Block,
        timestamp: u64,
        reverted_block_root: H256,
    ) -> Self {
        let block_param = MockBlockParam::new(
            ctx.mock_rollup.rollup_context.to_owned(),
            block_producer,
            parent_block,
            timestamp,
            reverted_block_root,
        );

        let safe_margin = MarginOfMockBlockSafity {
            remain_package_size: u64::MAX,
            prev_raw_block_size: 0,
        };

        OffChainCancelChallengeValidator {
            validator_context: ctx,
            safe_margin,
            block_param,
        }
    }

    pub fn reset(&mut self, parent_block: &L2Block, timestamp: u64, reverted_block_root: H256) {
        self.block_param
            .reset(parent_block, timestamp, reverted_block_root);

        self.safe_margin = MarginOfMockBlockSafity {
            remain_package_size: u64::MAX,
            prev_raw_block_size: 0,
        };
    }

    pub fn verify_withdrawal_request(
        &mut self,
        db: &mut StoreTransaction,
        mem_tree: &mut MemTree,
        req: WithdrawalRequestExtra,
    ) -> Result<Option<u64>> {
        let block_param = &mut self.block_param;
        let safe_margin = &mut self.safe_margin;
        let validator_ctx = &self.validator_context;

        let withdrawal_hash: ckb_types::H256 = req.hash().into();
        let dump_prefix = "withdrawal-signature";

        block_param.push_withdrawal_request(mem_tree, req)?;

        let max_cycles = self.validator_context.validator_config.verify_max_cycles
            - MARGIN_OF_MOCK_BLOCK_SAFITY_CYCLES;

        let mut tx_with_context = None;
        let mut verify = || -> Result<_> {
            let challenge = block_param.challenge_last_withdrawal(db, mem_tree)?;
            let mock_output = mock_tx::mock_cancel_challenge_tx(
                &validator_ctx.mock_rollup,
                challenge.global_state,
                challenge.challenge_target,
                challenge.verify_context,
                None,
            )?;

            tx_with_context = Some(TxWithContext::from(mock_output.clone()));

            safe_margin.check_and_update(
                challenge.raw_block_size,
                mock_output.tx.as_slice().len() as u64,
                RawBlockFlag::New,
            )?;

            let cycles = verify_tx(
                &validator_ctx.rollup_cell_deps,
                TxWithContext::from(mock_output),
                max_cycles,
            )?;

            Ok(Some(cycles))
        };

        let validator_config = &self.validator_context.validator_config;
        if !validator_config.verify_withdrawal_signature {
            return Ok(None);
        }

        let result = verify();
        if matches!(result, Result::Err(_)) {
            block_param.pop_withdrawal_request();

            if !validator_config.dump_tx_on_failure {
                return result;
            }

            if let Some(tx_with_context) = tx_with_context {
                dump_tx_to_file(
                    validator_ctx,
                    dump_prefix,
                    &withdrawal_hash.to_string(),
                    tx_with_context,
                );
            }
        }

        result
    }

    pub fn set_prev_txs_checkpoint(&mut self, checkpoint: H256) {
        self.block_param.set_prev_txs_checkpoint(checkpoint)
    }

    #[cfg(gw_challenge)]
    pub fn verify_transaction(
        &mut self,
        db: &StoreTransaction,
        mem_tree: &mut MemTree,
        tx: L2Transaction,
        run_result: &RunResult,
    ) -> Result<Option<VerifyTxCycles>> {
        let block_param = &mut self.block_param;
        let safe_margin = &mut self.safe_margin;
        let validator_ctx = &self.validator_context;

        let tx_hash: ckb_types::H256 = tx.hash().into();
        block_param.push_transaction(mem_tree, tx, run_result)?;

        let max_cycles = self.validator_context.validator_config.verify_max_cycles
            - MARGIN_OF_MOCK_BLOCK_SAFITY_CYCLES;

        let verify_signature = |tx_with_context: &mut Option<TxWithContext>,
                                safe_margin: &mut MarginOfMockBlockSafity,
                                raw_block_flag: &mut RawBlockFlag,
                                mem_tree: &mut MemTree|
         -> Result<_> {
            let challenge = block_param.challenge_last_tx_signature(db, mem_tree)?;
            let mock_output = mock_tx::mock_cancel_challenge_tx(
                &validator_ctx.mock_rollup,
                challenge.global_state,
                challenge.challenge_target,
                challenge.verify_context,
                None,
            )?;

            *tx_with_context = Some(TxWithContext::from(mock_output.clone()));

            safe_margin.check_and_update(
                challenge.raw_block_size,
                mock_output.tx.as_slice().len() as u64,
                *raw_block_flag,
            )?;
            if *raw_block_flag == RawBlockFlag::New {
                *raw_block_flag = RawBlockFlag::Prev;
            }

            let cycles = verify_tx(
                &validator_ctx.rollup_cell_deps,
                TxWithContext::from(mock_output),
                max_cycles,
            )?;

            Ok(Some(cycles))
        };

        let verify_execution = |tx_with_context: &mut Option<TxWithContext>,
                                safe_margin: &mut MarginOfMockBlockSafity,
                                raw_block_flag: &mut RawBlockFlag,
                                load_data_strategy: LoadDataStrategy,
                                mem_tree: &mut MemTree|
         -> Result<_> {
            let challenge = block_param.challenge_last_tx_execution(db, mem_tree, run_result)?;
            let mock_output = mock_tx::mock_cancel_challenge_tx(
                &validator_ctx.mock_rollup,
                challenge.global_state,
                challenge.challenge_target,
                challenge.verify_context,
                Some(load_data_strategy),
            )?;

            *tx_with_context = Some(TxWithContext::from(mock_output.clone()));

            safe_margin.check_and_update(
                challenge.raw_block_size,
                mock_output.tx.as_slice().len() as u64,
                *raw_block_flag,
            )?;
            if *raw_block_flag == RawBlockFlag::New {
                *raw_block_flag = RawBlockFlag::Prev;
            }

            let cycles = verify_tx(
                &validator_ctx.rollup_cell_deps,
                TxWithContext::from(mock_output),
                max_cycles,
            )?;

            Ok(Some(cycles))
        };

        let validator_config = &self.validator_context.validator_config;
        if !validator_config.verify_tx_signature && !validator_config.verify_tx_execution {
            return Ok(None);
        }

        let mut tx_with_context = None;
        let mut dump_prefix = "tx-signature";
        let mut raw_block_flag = RawBlockFlag::New;
        let mut cycles = VerifyTxCycles {
            signature: None,
            execution_by_witness: None,
            execution_by_celldep: None,
        };

        let verify = |mem_tree: &mut MemTree| -> Result<_> {
            if validator_config.verify_tx_signature {
                cycles.signature = verify_signature(
                    &mut tx_with_context,
                    safe_margin,
                    &mut raw_block_flag,
                    mem_tree,
                )?;
            }

            if validator_config.verify_tx_execution {
                let mut by_witness = || -> Result<_> {
                    dump_prefix = "tx-execution-with-witness-data-loader";
                    cycles.execution_by_witness = verify_execution(
                        &mut tx_with_context,
                        safe_margin,
                        &mut raw_block_flag,
                        LoadDataStrategy::Witness,
                        mem_tree,
                    )?;

                    Ok(())
                };

                match by_witness() {
                    Ok(_) => return Ok(Some(cycles)),
                    Err(_) if validator_config.dump_tx_on_failure => {
                        if let Some(tx_with_context) = tx_with_context.take() {
                            dump_tx_to_file(
                                validator_ctx,
                                dump_prefix,
                                &tx_hash.to_string(),
                                tx_with_context,
                            );
                        }
                    }
                    Err(err) => log::warn!("offchain validator: verify tx execution {}", err),
                }

                // Try again use cell dep
                dump_prefix = "tx-execution-with-celldep-data-loader";
                cycles.execution_by_witness = verify_execution(
                    &mut tx_with_context,
                    safe_margin,
                    &mut raw_block_flag,
                    LoadDataStrategy::CellDep,
                    mem_tree,
                )?;
            }

            Ok(Some(cycles))
        };

        let result = verify(mem_tree);
        if matches!(result, Result::Err(_)) {
            block_param.pop_transaction();

            if !validator_config.dump_tx_on_failure {
                return result;
            }

            if let Some(tx_with_context) = tx_with_context {
                dump_tx_to_file(
                    validator_ctx,
                    dump_prefix,
                    &tx_hash.to_string(),
                    tx_with_context,
                );
            }
        }

        result
    }
}

// MarginOfMockBlockSafity track mock cancel challenge tx's size to ensure
// withdrawal/transaction pushed later won't break packaged ones.
//
// NOTE: OffChain cancel challenge verification bases on partial block. This
// result in smaller tx's size and cycles than full block.
//
// Tx size is affected by withdrawal/transaction proof and state_checkpoint_list.
#[derive(Debug)]
struct MarginOfMockBlockSafity {
    remain_package_size: u64,
    prev_raw_block_size: u64,
}

#[allow(dead_code)]
#[derive(Clone, Copy, PartialEq, Eq)]
enum RawBlockFlag {
    New,
    Prev,
}

impl MarginOfMockBlockSafity {
    fn check_and_update(
        &mut self,
        raw_block_size: u64,
        tx_size: u64,
        raw_block: RawBlockFlag,
    ) -> Result<()> {
        if tx_size > MARGIN_OF_MOCK_BLOCK_SAFITY_TX_SIZE_LIMIT {
            bail!(
                "offchain cancel challenge tx size exceeded {}, got {}",
                MARGIN_OF_MOCK_BLOCK_SAFITY_TX_SIZE_LIMIT,
                tx_size
            );
        }
        assert!(raw_block_size <= MARGIN_OF_MOCK_BLOCK_SAFITY_TX_SIZE_LIMIT);

        if self.remain_package_size == u64::MAX {
            self.remain_package_size = MARGIN_OF_MOCK_BLOCK_SAFITY_TX_SIZE_LIMIT - tx_size;
            self.prev_raw_block_size = raw_block_size;
            return Ok(());
        }

        // Check size for packaged withdrawals and txs
        let new_remain_package_size = match raw_block {
            RawBlockFlag::New => {
                assert!(
                    raw_block_size > self.prev_raw_block_size,
                    "checkpoint should increase raw block size"
                );

                let diff_size = raw_block_size - self.prev_raw_block_size;
                match self.remain_package_size.checked_sub(diff_size) {
                    Some(size) => size,
                    None => bail!("reach max block size limit"),
                }
            }
            RawBlockFlag::Prev => self.remain_package_size,
        };

        // Update size
        let tx_remain_package_size = MARGIN_OF_MOCK_BLOCK_SAFITY_TX_SIZE_LIMIT - tx_size;
        self.remain_package_size = u64::min(new_remain_package_size, tx_remain_package_size);
        self.prev_raw_block_size = raw_block_size;

        Ok(())
    }
}

async fn resolve_cell_deps(
    rpc_client: &RPCClient,
    deps: Vec<CellDep>,
) -> Result<Vec<InputCellInfo>> {
    let mut flatten_deps: HashSet<CellDep> = HashSet::with_capacity(deps.len());
    for dep in deps {
        let cell_deps = resolve_dep_group(rpc_client, &dep).await?;
        flatten_deps.insert(dep);
        flatten_deps.extend(cell_deps);
    }

    let mut resolved_deps = Vec::with_capacity(flatten_deps.len());
    for dep in flatten_deps {
        let dep_cell = {
            let query = rpc_client.get_cell(dep.out_point()).await?;
            query
                .and_then(|q| q.cell)
                .ok_or_else(|| anyhow!("can't find dep cell"))?
        };
        resolved_deps.push(into_input_cell_info(dep_cell));
    }

    Ok(resolved_deps)
}

async fn resolve_dep_group(rpc_client: &RPCClient, dep: &CellDep) -> Result<Vec<CellDep>> {
    // return dep
    if dep.dep_type() == DepType::Code.into() {
        return Ok(vec![]);
    }

    // parse dep group
    let cell = {
        let query = rpc_client.get_cell(dep.out_point()).await?;
        query
            .and_then(|q| q.cell)
            .ok_or_else(|| anyhow!("can't find dep group cell"))?
    };

    let out_points =
        OutPointVec::from_slice(&cell.data).map_err(|_| anyhow!("invalid dep group"))?;

    let into_dep = |out_point: OutPoint| -> CellDep {
        CellDep::new_builder()
            .out_point(out_point)
            .dep_type(DepType::Code.into())
            .build()
    };

    Ok(out_points.into_iter().map(into_dep).collect())
}

fn into_input_cell_info(cell_info: CellInfo) -> InputCellInfo {
    InputCellInfo {
        input: CellInput::new_builder()
            .previous_output(cell_info.out_point.clone())
            .build(),
        cell: cell_info,
    }
}

impl From<MockOutput> for TxWithContext {
    fn from(output: MockOutput) -> Self {
        TxWithContext {
            cell_deps: output.cell_deps,
            inputs: output.inputs,
            tx: output.tx,
        }
    }
}

fn dump_tx_to_file(
    validator_context: &OffChainValidatorContext,
    prefix: &str,
    origin_hash: &str,
    tx_with_context: TxWithContext,
) {
    let dump = || -> Result<_> {
        let debug_config = &validator_context.debug_config;
        let dir = debug_config.debug_tx_dump_path.as_path();
        create_dir_all(&dir)?;

        let mut dump_path = PathBuf::new();
        dump_path.push(dir);

        let tx = dump_tx(&validator_context.rollup_cell_deps, tx_with_context)?;
        let dump_filename = format!("{}-{}-offchain-cancel-tx.json", prefix, origin_hash);
        dump_path.push(dump_filename);

        let json_tx = serde_json::to_string_pretty(&tx)?;
        log::info!("dump cancel tx from {} to {:?}", origin_hash, dump_path);
        write(dump_path, json_tx)?;

        Ok(())
    };

    if let Err(err) = dump() {
        log::error!("unable to dump offchain cancel challenge tx {}", err);
    }
}
