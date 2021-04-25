use crate::rpc_client::RPCClient;
use crate::types::CellInfo;
use crate::types::InputCellInfo;

use anyhow::{anyhow, Result};
use ckb_types::{
    bytes::Bytes,
    prelude::{Builder, Entity},
};
use gw_config::BlockProducerConfig;
use gw_generator::RollupContext;
use gw_types::{
    core::{DepType, ScriptHashType},
    packed::{
        CellDep, CellInput, CellOutput, GlobalState, L2Block, OutPoint, Script, StakeLockArgs,
        Transaction,
    },
    prelude::{Pack, Unpack},
};
use parking_lot::Mutex;

use std::iter::FromIterator;
use std::sync::Arc;

pub struct StakeCell {
    info: InputCellInfo,
    block_number: u64,
}

pub struct GeneratedStake {
    pub deps: Vec<CellDep>,
    pub inputs: Vec<InputCellInfo>,
    pub output: CellOutput,
    pub output_data: Bytes,
}

pub struct Stake {
    cells: Arc<Mutex<Vec<StakeCell>>>,
}

impl Stake {
    pub fn new() -> Self {
        Stake {
            cells: Default::default(),
        }
    }

    pub fn add_stake(&self, rollup_context: &RollupContext, tx: Transaction) {
        let stake_script_type_hash = rollup_context.rollup_config.stake_script_type_hash();
        let mut tx_outputs = tx.raw().outputs().into_iter().enumerate();
        let (stake_idx, stake_output) = match tx_outputs
            .find(|(_, output)| output.lock().code_hash() == stake_script_type_hash)
        {
            None => return,
            Some(output) => output,
        };

        let stake_block_number = {
            let args: Bytes = stake_output.lock().args().unpack();
            let stake_lock_args = StakeLockArgs::new_unchecked(args.slice(32..));
            stake_lock_args.stake_block_number().unpack()
        };

        let out_point = OutPoint::new_builder()
            .tx_hash(tx.hash().pack())
            .index(stake_idx.pack())
            .build();
        let output_data = tx.raw().outputs_data().get(stake_idx).unwrap_or_default();

        let cell = CellInfo {
            out_point: out_point.clone(),
            output: stake_output,
            data: output_data.as_bytes(),
        };
        let input = CellInput::new_builder().previous_output(out_point).build();
        let input_cell = InputCellInfo { input, cell };

        let stake_cell = StakeCell {
            info: input_cell,
            block_number: stake_block_number,
        };
        self.cells.lock().push(stake_cell)
    }

    // TODO: use unlocked live stake cell before godwoken process start
    pub async fn generate(
        &self,
        rollup_cell: &CellInfo,
        rollup_context: &RollupContext,
        block: &L2Block,
        block_producer_config: &BlockProducerConfig,
        rpc_client: &RPCClient,
        lock_script: Script,
    ) -> Result<GeneratedStake> {
        let lock_args = {
            let owner_lock_hash = lock_script.hash();

            let stake_lock_args = StakeLockArgs::new_builder()
                .owner_lock_hash(owner_lock_hash.pack())
                .stake_block_number(block.raw().number())
                .build();

            let rollup_type_hash = rollup_context.rollup_script_hash.as_slice().iter().cloned();
            Bytes::from_iter(rollup_type_hash.chain(stake_lock_args.as_slice().iter().cloned()))
        };

        let lock = Script::new_builder()
            .code_hash(rollup_context.rollup_config.stake_script_type_hash())
            .hash_type(ScriptHashType::Type.into())
            .args(lock_args.pack())
            .build();

        if let Some(unlocked_stake) = self.unlocked_stake(rollup_cell)? {
            let stake_lock_dep = block_producer_config.stake_cell_lock_dep.clone();
            let rollup_cell_dep = CellDep::new_builder()
                .out_point(rollup_cell.out_point.to_owned())
                .dep_type(DepType::Code.into())
                .build();

            let stake_cell = CellOutput::new_builder()
                .capacity(unlocked_stake.cell.output.capacity())
                .lock(lock)
                .build();

            let generated_stake = GeneratedStake {
                deps: vec![stake_lock_dep.into(), rollup_cell_dep],
                inputs: vec![unlocked_stake],
                output: stake_cell,
                output_data: Bytes::new(),
            };

            return Ok(generated_stake);
        }

        // No unlocked stake, collect free ckb cells to generate one
        let stake_capacity = {
            let required_staking_capacity = rollup_context
                .rollup_config
                .required_staking_capacity()
                .unpack();

            assert!(lock.as_slice().len() < u64::max_value() as usize);
            let min_capacity = (8u64 + lock.as_slice().len() as u64) * 100000000;

            if required_staking_capacity <= min_capacity {
                min_capacity
            } else {
                required_staking_capacity
            }
        };

        let payment_cells = rpc_client
            .query_payment_cells(lock_script.clone(), stake_capacity)
            .await?;
        if payment_cells.is_empty() {
            return Err(anyhow!("no cells to generate stake cell"));
        }

        let input_cells = payment_cells
            .into_iter()
            .map(|cell| {
                let input = CellInput::new_builder()
                    .previous_output(cell.out_point.clone())
                    .build();
                InputCellInfo { input, cell }
            })
            .collect();

        let stake_cell = CellOutput::new_builder()
            .capacity(stake_capacity.pack())
            .lock(lock)
            .build();

        let generated_stake = GeneratedStake {
            deps: vec![],
            inputs: input_cells,
            output: stake_cell,
            output_data: Bytes::new(),
        };

        Ok(generated_stake)
    }

    fn unlocked_stake(&self, rollup_cell: &CellInfo) -> Result<Option<InputCellInfo>> {
        let global_state = GlobalState::from_slice(&rollup_cell.data)
            .map_err(|_| anyhow!("parse rollup cell global state"))?;
        let last_finalized_block_number = global_state.last_finalized_block_number().unpack();

        let mut cells = self.cells.lock();
        match cells.first() {
            Some(cell) if cell.block_number <= last_finalized_block_number => {
                Ok(Some(cells.remove(0).info))
            }
            _ => Ok(None),
        }
    }
}
