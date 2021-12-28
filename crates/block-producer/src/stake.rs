use anyhow::Result;
use ckb_types::{
    bytes::Bytes,
    prelude::{Builder, Entity},
};
use gw_config::ContractsCellDep;
use gw_rpc_client::rpc_client::RPCClient;
use gw_types::{
    core::{DepType, ScriptHashType},
    offchain::{CellInfo, InputCellInfo, RollupContext},
    packed::{CellDep, CellInput, CellOutput, L2Block, Script, StakeLockArgs},
    prelude::{Pack, Unpack},
};

pub struct GeneratedStake {
    pub deps: Vec<CellDep>,
    pub inputs: Vec<InputCellInfo>,
    pub output: CellOutput,
    pub output_data: Bytes,
}

pub async fn generate(
    rollup_cell: &CellInfo,
    rollup_context: &RollupContext,
    block: &L2Block,
    contracts_dep: &ContractsCellDep,
    rpc_client: &RPCClient,
    lock_script: Script,
) -> Result<GeneratedStake> {
    let owner_lock_hash = lock_script.hash();
    let lock_args: Bytes = {
        let stake_lock_args = StakeLockArgs::new_builder()
            .owner_lock_hash(owner_lock_hash.pack())
            .stake_block_number(block.raw().number())
            .build();

        let rollup_type_hash = rollup_context.rollup_script_hash.as_slice().iter();
        rollup_type_hash
            .chain(stake_lock_args.as_slice().iter())
            .cloned()
            .collect()
    };

    let lock = Script::new_builder()
        .code_hash(rollup_context.rollup_config.stake_script_type_hash())
        .hash_type(ScriptHashType::Type.into())
        .args(lock_args.pack())
        .build();

    let required_staking_capacity = rollup_context
        .rollup_config
        .required_staking_capacity()
        .unpack();
    if let Some(unlocked_stake) = rpc_client
        .query_stake(
            rollup_context,
            owner_lock_hash,
            required_staking_capacity,
            None,
        )
        .await?
    {
        let stake_lock_dep = contracts_dep.stake_cell_lock.clone();
        let rollup_cell_dep = CellDep::new_builder()
            .out_point(rollup_cell.out_point.to_owned())
            .dep_type(DepType::Code.into())
            .build();

        let stake_cell = CellOutput::new_builder()
            .capacity(unlocked_stake.output.capacity())
            .lock(lock)
            .build();

        let input_unlocked_stake = InputCellInfo {
            input: CellInput::new_builder()
                .previous_output(unlocked_stake.out_point.clone())
                .build(),
            cell: unlocked_stake,
        };

        let generated_stake = GeneratedStake {
            deps: vec![stake_lock_dep.into(), rollup_cell_dep],
            inputs: vec![input_unlocked_stake],
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

    let stake_cell = CellOutput::new_builder()
        .capacity(stake_capacity.pack())
        .lock(lock)
        .build();

    let generated_stake = GeneratedStake {
        deps: vec![],
        inputs: vec![],
        output: stake_cell,
        output_data: Bytes::new(),
    };

    Ok(generated_stake)
}
