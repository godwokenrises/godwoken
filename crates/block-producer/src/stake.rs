use anyhow::Result;
use ckb_types::{
    bytes::Bytes,
    prelude::{Builder, Entity},
};
use gw_config::ContractsCellDep;
use gw_rpc_client::{
    indexer_client::CKBIndexerClient,
    indexer_types::{Order, SearchKey, SearchKeyFilter},
    rpc_client::RPCClient,
};
use gw_types::core::Timepoint;
use gw_types::offchain::CompatibleFinalizedTimepoint;
use gw_types::{
    core::ScriptHashType,
    offchain::{CellInfo, InputCellInfo},
    packed::{CellDep, CellInput, CellOutput, L2Block, Script, StakeLockArgs, StakeLockArgsReader},
    prelude::*,
};
use gw_utils::local_cells::{
    collect_local_and_indexer_cells, CollectLocalAndIndexerCursor, LocalCellsManager,
};
use gw_utils::{finalized_timepoint, RollupContext};

pub struct GeneratedStake {
    pub deps: Vec<CellDep>,
    pub inputs: Vec<InputCellInfo>,
    pub output: CellOutput,
    pub output_data: Bytes,
}

pub async fn generate(
    _rollup_cell: &CellInfo,
    rollup_context: &RollupContext,
    block: &L2Block,
    contracts_dep: &ContractsCellDep,
    rpc_client: &RPCClient,
    lock_script: Script,
    local_cells_manager: &LocalCellsManager,
) -> Result<GeneratedStake> {
    let owner_lock_hash = lock_script.hash();
    let stake_block_timepoint = finalized_timepoint(
        &rollup_context.rollup_config,
        &rollup_context.fork_config,
        block.raw().number().unpack(),
        block.raw().timestamp().unpack(),
    );
    let lock_args: Bytes = {
        let stake_lock_args = StakeLockArgs::new_builder()
            .owner_lock_hash(owner_lock_hash.pack())
            .stake_block_timepoint(stake_block_timepoint.full_value().pack())
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
    if let Some(unlocked_stake) = query_stake(
        &rpc_client.indexer,
        rollup_context,
        owner_lock_hash,
        required_staking_capacity,
        None,
        local_cells_manager,
    )
    .await?
    {
        let stake_lock_dep = contracts_dep.stake_cell_lock.clone();

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
            deps: vec![stake_lock_dep.into()],
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

/// query stake
///
/// Returns a finalized stake_state_cell if `compatible_finalize_timepoint_opt` is some,
/// otherwise returns a random stake_state_cell.
pub async fn query_stake(
    client: &CKBIndexerClient,
    rollup_context: &RollupContext,
    owner_lock_hash: [u8; 32],
    required_staking_capacity: u64,
    compatible_finalize_timepoint_opt: Option<CompatibleFinalizedTimepoint>,
    local_cells_manager: &LocalCellsManager,
) -> Result<Option<CellInfo>> {
    let lock = Script::new_builder()
        .code_hash(rollup_context.rollup_config.stake_script_type_hash())
        .hash_type(ScriptHashType::Type.into())
        .args(rollup_context.rollup_script_hash.as_slice().pack())
        .build();

    let search_key = SearchKey::with_lock(lock).with_filter(Some(SearchKeyFilter {
        output_capacity_range: Some([required_staking_capacity.into(), u64::MAX.into()]),
        ..Default::default()
    }));
    let order = Order::Desc;

    let mut stake_cell = None;
    let mut cursor = CollectLocalAndIndexerCursor::Local;

    while stake_cell.is_none() && !cursor.is_ended() {
        let cells = collect_local_and_indexer_cells(
            local_cells_manager,
            client,
            &search_key,
            &order,
            Some(1),
            &mut cursor,
        )
        .await?;

        stake_cell = cells.into_iter().find(|cell| {
            let args = cell.output.as_reader().lock().args().raw_data();
            let stake_lock_args = match StakeLockArgsReader::from_slice(&args[32..]) {
                Ok(r) => r,
                Err(_) => return false,
            };
            match &compatible_finalize_timepoint_opt {
                Some(compatible_finalized_timepoint) => {
                    compatible_finalized_timepoint.is_finalized(&Timepoint::from_full_value(
                        stake_lock_args.stake_block_timepoint().unpack(),
                    )) && stake_lock_args.owner_lock_hash().as_slice() == owner_lock_hash
                }
                None => stake_lock_args.owner_lock_hash().as_slice() == owner_lock_hash,
            }
        });
    }

    Ok(stake_cell)
}
