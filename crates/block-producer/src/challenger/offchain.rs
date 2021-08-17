use crate::{
    challenger::offchain::verify_tx::dump_tx,
    poa::PoA,
    rpc_client::RPCClient,
    types::{CellInfo, InputCellInfo},
    utils::CKBGenesisInfo,
    wallet::Wallet,
};

use anyhow::{anyhow, bail, Result};
use ckb_chain_spec::consensus::MAX_BLOCK_BYTES;
use gw_common::{state::State, H256};
use gw_config::{BlockProducerConfig, DebugConfig};
use gw_generator::RollupContext;
use gw_store::{state_db::StateDBTransaction, transaction::StoreTransaction};
use gw_types::{
    core::DepType,
    offchain::RunResult,
    packed::{
        AccountMerkleState, CellDep, CellInput, L2Block, L2Transaction, OutPoint, OutPointVec,
        WithdrawalRequest,
    },
    prelude::*,
};

use std::{
    collections::{HashMap, HashSet},
    fs::{create_dir_all, write},
    path::PathBuf,
    sync::Arc,
};

pub mod mock_block;
pub mod mock_poa;
pub mod mock_tx;
pub mod verify_tx;
use mock_block::MockBlockParam;
use mock_tx::MockRollup;
use verify_tx::RollupCellDeps;

use self::{
    mock_poa::MockPoA,
    mock_tx::MockOutput,
    verify_tx::{verify_tx, TxWithContext},
};

const MAX_TX_WITHDRAWAL_PROOF_SIZE: u64 = 32 * 33 + 1;
// TODO: Relax limit
const MARGIN_OF_MOCK_BLOCK_SAFITY_MAX_CYCLES: u64 = 65_000_000;
const MARGIN_OF_MOCK_BLOCK_SAFITY_TX_SIZE_LIMIT: u64 =
    MAX_BLOCK_BYTES - MAX_TX_WITHDRAWAL_PROOF_SIZE;

#[derive(Clone)]
pub struct OffChainContext {
    debug_config: Arc<DebugConfig>,
    rollup_cell_deps: RollupCellDeps,
    mock_rollup: Arc<MockRollup>,
    mock_poa: Arc<MockPoA>,
}

impl OffChainContext {
    pub async fn build(
        rpc_client: &RPCClient,
        poa: &PoA,
        rollup_context: RollupContext,
        wallet: Wallet,
        config: BlockProducerConfig,
        debug_config: DebugConfig,
        ckb_genesis_info: CKBGenesisInfo,
        builtin_load_data: HashMap<H256, CellDep>,
    ) -> Result<Self> {
        let rollup_cell = {
            let query = rpc_client.query_rollup_cell().await?;
            into_input_cell_info(query.ok_or_else(|| anyhow!("can't found rollup cell"))?)
        };
        let mock_poa = Arc::new(MockPoA::build(rpc_client, poa, &rollup_cell, &config).await?);

        let rollup_output = rollup_cell.cell.output;
        let mock_rollup = {
            let mock = MockRollup {
                rollup_output,
                rollup_context,
                wallet,
                config,
                ckb_genesis_info,
                builtin_load_data,
            };
            Arc::new(mock)
        };

        let rollup_deps: Vec<CellDep> = {
            let mut deps = vec![
                mock_rollup.config.rollup_cell_type_dep.clone().into(),
                mock_rollup.config.rollup_config_cell_dep.clone().into(),
                mock_rollup.config.challenge_cell_lock_dep.clone().into(),
                mock_rollup.ckb_genesis_info.sighash_dep().into(),
            ];
            deps.extend({
                let contract_deps = mock_rollup.config.allowed_contract_deps.values();
                contract_deps.cloned().map(CellDep::from)
            });
            deps.extend({
                let eoa_deps = mock_rollup.config.allowed_eoa_deps.values();
                eoa_deps.cloned().map(CellDep::from)
            });
            deps.extend(mock_rollup.builtin_load_data.values().cloned());
            deps.extend(vec![mock_poa.lock_dep.clone(), mock_poa.state_dep.clone()]);

            deps
        };

        let debug_config = Arc::new(debug_config);
        let resolved_rollup_deps = resolve_cell_deps(rpc_client, rollup_deps).await?;
        let rollup_cell_deps = RollupCellDeps::new(resolved_rollup_deps);

        Ok(OffChainContext {
            debug_config,
            rollup_cell_deps,
            mock_rollup,
            mock_poa,
        })
    }
}

pub struct ValidateTxCycles {
    pub signature: u64,
    pub execution: u64,
}

pub struct OffChainCancelChallengeValidator {
    ctx: OffChainContext,
    safe_margin: MarginOfMockBlockSafity,
    block_param: MockBlockParam,
}

impl OffChainCancelChallengeValidator {
    pub fn new(ctx: OffChainContext, parent_block: &L2Block, reverted_block_root: H256) -> Self {
        let block_param = MockBlockParam::new(
            &ctx.mock_rollup.rollup_context,
            parent_block,
            reverted_block_root,
        );

        let safe_margin = MarginOfMockBlockSafity {
            remain_package_size: u64::MAX,
            prev_raw_block_size: 0,
        };

        OffChainCancelChallengeValidator {
            ctx,
            safe_margin,
            block_param,
        }
    }

    pub fn validate_withdrawal_request(
        &mut self,
        db: &StoreTransaction,
        state_db: &StateDBTransaction<'_>,
        req: WithdrawalRequest,
    ) -> Result<u64> {
        let state = state_db.account_state_tree()?;
        let post_account = AccountMerkleState::new_builder()
            .merkle_root(state.calculate_root()?.pack())
            .count(state.get_account_count()?.pack())
            .build();

        let block_param = &mut self.block_param;
        let safe_margin = &mut self.safe_margin;
        let offchain_ctx = &self.ctx;
        block_param.push_withdrawal_request(req, post_account);

        let mut tx_with_context = None;
        let mut verify = || -> Result<_> {
            let challenge = block_param.challenge_last_withdrawal(db, state_db)?;
            let mock_output = mock_tx::mock_cancel_challenge_tx(
                &offchain_ctx.mock_rollup,
                &offchain_ctx.mock_poa,
                challenge.global_state,
                challenge.challenge_target,
                challenge.verify_context,
            )?;

            tx_with_context = Some(TxWithContext::from(mock_output.clone()));

            safe_margin.check_and_update(
                challenge.raw_block_size,
                mock_output.tx.as_slice().len() as u64,
                RawBlock::New,
            )?;

            let cycles = verify_tx(
                &offchain_ctx.rollup_cell_deps,
                TxWithContext::from(mock_output),
                MARGIN_OF_MOCK_BLOCK_SAFITY_MAX_CYCLES,
            )?;

            Ok(cycles)
        };

        let result = verify();
        if matches!(result, Result::Err(_)) {
            block_param.pop_withdrawal_request();
            if let Some(tx_with_context) = tx_with_context {
                self.dump_tx_to_file(tx_with_context);
            }
        }

        result
    }

    pub fn set_prev_txs_checkpoint(&mut self, checkpoint: H256) {
        self.block_param.set_prev_txs_checkpoint(checkpoint)
    }

    pub fn validate_tx(
        &mut self,
        db: &StoreTransaction,
        state_db: &StateDBTransaction<'_>,
        tx: L2Transaction,
        run_result: &RunResult,
    ) -> Result<ValidateTxCycles> {
        let block_param = &mut self.block_param;
        let safe_margin = &mut self.safe_margin;

        let offchain_ctx = &self.ctx;
        block_param.push_transaction(db, state_db, tx, run_result)?;

        let mut tx_with_context = None;
        let mut verify = || -> Result<_> {
            let mut cycles = ValidateTxCycles {
                signature: 0,
                execution: 0,
            };

            let challenge = block_param.challenge_last_tx_signature(db, state_db)?;
            let mock_output = mock_tx::mock_cancel_challenge_tx(
                &offchain_ctx.mock_rollup,
                &offchain_ctx.mock_poa,
                challenge.global_state,
                challenge.challenge_target,
                challenge.verify_context,
            )?;

            tx_with_context = Some(TxWithContext::from(mock_output.clone()));

            safe_margin.check_and_update(
                challenge.raw_block_size,
                mock_output.tx.as_slice().len() as u64,
                RawBlock::New,
            )?;

            cycles.signature = verify_tx(
                &offchain_ctx.rollup_cell_deps,
                TxWithContext::from(mock_output),
                MARGIN_OF_MOCK_BLOCK_SAFITY_MAX_CYCLES,
            )?;

            let challenge = block_param.challenge_last_tx_execution(db, state_db, run_result)?;
            let mock_output = mock_tx::mock_cancel_challenge_tx(
                &offchain_ctx.mock_rollup,
                &offchain_ctx.mock_poa,
                challenge.global_state,
                challenge.challenge_target,
                challenge.verify_context,
            )?;

            tx_with_context = Some(TxWithContext::from(mock_output.clone()));

            safe_margin.check_and_update(
                challenge.raw_block_size,
                mock_output.tx.as_slice().len() as u64,
                RawBlock::Prev,
            )?;

            cycles.execution = verify_tx(
                &offchain_ctx.rollup_cell_deps,
                TxWithContext::from(mock_output),
                MARGIN_OF_MOCK_BLOCK_SAFITY_MAX_CYCLES,
            )?;

            Ok(cycles)
        };

        let result = verify();
        if matches!(result, Result::Err(_)) {
            block_param.pop_transaction();
            if let Some(tx_with_context) = tx_with_context {
                self.dump_tx_to_file(tx_with_context);
            }
        }

        result
    }

    fn dump_tx_to_file(&self, tx_with_context: TxWithContext) {
        let dump = || -> Result<_> {
            let dir = self.ctx.debug_config.debug_tx_dump_path.as_path();
            create_dir_all(&dir)?;

            let mut dump_path = PathBuf::new();
            dump_path.push(dir);

            let tx_hash: ckb_types::H256 = tx_with_context.tx.hash().into();
            let tx = dump_tx(&self.ctx.rollup_cell_deps, tx_with_context)?;
            dump_path.push(format!("{}-mock-tx.json", tx_hash));

            let json_tx = serde_json::to_string_pretty(&tx)?;
            log::info!("dump offchain cancel tx {} to {:?}", tx_hash, dump_path);
            write(dump_path, json_tx)?;

            Ok(())
        };

        if let Err(err) = dump() {
            log::error!("unable to dump offchain cancel challenge tx {}", err);
        }
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

enum RawBlock {
    New,
    Prev,
}

impl MarginOfMockBlockSafity {
    fn check_and_update(
        &mut self,
        raw_block_size: u64,
        tx_size: u64,
        raw_block: RawBlock,
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
            RawBlock::New => {
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
            RawBlock::Prev => self.remain_package_size,
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
            query.ok_or_else(|| anyhow!("can't find dep cell"))?
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
        query.ok_or_else(|| anyhow!("can't find dep group cell"))?
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
