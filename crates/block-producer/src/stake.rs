use anyhow::Result;
use ckb_types::{
    bytes::Bytes,
    prelude::{Builder, Entity},
};
use gw_config::ContractsCellDep;
use gw_rpc_client::{
    indexer_client::CKBIndexerClient,
    indexer_types::{Cell, Order, ScriptType, SearchKey, SearchKeyFilter},
    rpc_client::RPCClient,
};
use gw_types::{
    core::ScriptHashType,
    offchain::{CellInfo, InputCellInfo, RollupContext},
    packed::{
        CellDep, CellInput, CellOutput, L2Block, OutPoint, Script, StakeLockArgs,
        StakeLockArgsReader,
    },
    prelude::*,
};
use gw_utils::local_cells::LocalCellsManager;

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
        log::info!("using stake cell input: {:?}", unlocked_stake.out_point);
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
/// return cell which stake_block_number is less than last_finalized_block_number if the args isn't none
/// otherwise return stake cell randomly
pub async fn query_stake(
    client: &CKBIndexerClient,
    rollup_context: &RollupContext,
    owner_lock_hash: [u8; 32],
    required_staking_capacity: u64,
    last_finalized_block_number: Option<u64>,
    local_cells_manager: &LocalCellsManager,
) -> Result<Option<CellInfo>> {
    let lock = Script::new_builder()
        .code_hash(rollup_context.rollup_config.stake_script_type_hash())
        .hash_type(ScriptHashType::Type.into())
        .args(rollup_context.rollup_script_hash.as_slice().pack())
        .build();

    // First try local live stake cells.
    // TODO: check lock args, finality.
    let c = local_cells_manager.local_live().find(|c| {
        c.output.lock() == lock && c.output.capacity().unpack() >= required_staking_capacity
    });
    if c.is_some() {
        return Ok(c.cloned());
    }

    let search_key = SearchKey {
        script: {
            let lock = ckb_types::packed::Script::new_unchecked(lock.as_bytes());
            lock.into()
        },
        script_type: ScriptType::Lock,
        filter: Some(SearchKeyFilter {
            script: None,
            output_data_len_range: None,
            output_capacity_range: Some([required_staking_capacity.into(), u64::MAX.into()]),
            block_range: None,
        }),
    };
    let order = Order::Desc;

    let mut stake_cell = None;
    let mut cursor = None;

    while stake_cell.is_none() {
        let cells = client.get_cells(&search_key, &order, None, &cursor).await?;

        if cells.last_cursor.is_empty() {
            log::debug!("no unlocked stake");
            return Ok(None);
        }
        cursor = Some(cells.last_cursor);

        stake_cell = cells.objects.into_iter().find(|cell| {
            let out_point = {
                let out_point: ckb_types::packed::OutPoint = cell.out_point.clone().into();
                OutPoint::new_unchecked(out_point.as_bytes())
            };
            if local_cells_manager.is_dead(&out_point) {
                return false;
            }
            let args = cell.output.lock.args.clone().into_bytes();
            let stake_lock_args = match StakeLockArgsReader::verify(&args[32..], false) {
                Ok(()) => StakeLockArgs::new_unchecked(args.slice(32..)),
                Err(_) => return false,
            };
            match last_finalized_block_number {
                Some(last_finalized_block_number) => {
                    stake_lock_args.stake_block_number().unpack() <= last_finalized_block_number
                        && stake_lock_args.owner_lock_hash().as_slice() == owner_lock_hash
                }
                None => stake_lock_args.owner_lock_hash().as_slice() == owner_lock_hash,
            }
        });
    }

    let fetch_cell_info = |cell: Cell| {
        let out_point = {
            let out_point: ckb_types::packed::OutPoint = cell.out_point.into();
            OutPoint::new_unchecked(out_point.as_bytes())
        };
        let output = {
            let output: ckb_types::packed::CellOutput = cell.output.into();
            CellOutput::new_unchecked(output.as_bytes())
        };
        let data = cell.output_data.into_bytes();

        CellInfo {
            out_point,
            output,
            data,
        }
    };

    Ok(stake_cell.map(fetch_cell_info))
}
