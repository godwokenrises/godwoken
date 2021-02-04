use super::*;
use crate::tests::utils::layer1::build_simple_tx;
use ckb_types::{
    packed::CellInput,
    prelude::{Pack as CKBPack, Unpack},
};
use gw_chain::testing_tools::setup_chain;
use gw_chain::{chain::ProduceBlockParam, mem_pool::PackageParam};
use gw_types::{
    bytes::Bytes,
    core::ScriptHashType,
    packed::{
        RollupAction, RollupActionUnion, RollupConfig, RollupSubmitBlock, Script, StakeLockArgs,
    },
};

#[test]
fn test_submit_block() {
    let rollup_type_script = {
        Script::new_builder()
            .code_hash(Pack::pack(&*STATE_VALIDATOR_CODE_HASH))
            .hash_type(ScriptHashType::Data.into())
            .build()
    };
    // rollup lock & config
    let stake_lock_type = build_type_id_script(b"stake_lock_type_id");
    let stake_script_type_hash: [u8; 32] = stake_lock_type.calc_script_hash().unpack();
    let rollup_config = RollupConfig::new_builder()
        .stake_script_type_hash(Pack::pack(&stake_script_type_hash))
        .build();
    // setup chain
    let chain = setup_chain(rollup_type_script.clone(), rollup_config.clone());
    // deploy scripts
    let param = CellContextParam {
        stake_lock_type: stake_lock_type.clone(),
        ..Default::default()
    };
    let mut ctx = CellContext::new(&rollup_config, param);
    let stake_capacity = 10000_00000000u64;
    let input_stake_cell = {
        let cell = build_stake_cell(
            &rollup_type_script.hash(),
            &stake_script_type_hash,
            stake_capacity,
            StakeLockArgs::default(),
        );
        let out_point = ctx.insert_cell(cell, Bytes::default());
        CellInput::new_builder().previous_output(out_point).build()
    };
    let output_stake_cell = {
        let lock_args = StakeLockArgs::new_builder()
            .stake_block_number(Pack::pack(&1))
            .build();
        build_stake_cell(
            &rollup_type_script.hash(),
            &stake_script_type_hash,
            stake_capacity,
            lock_args,
        )
    };
    // create a rollup cell
    let capacity = 1000_00000000u64;
    let spend_cell = build_always_success_cell(capacity, None);
    let rollup_cell = build_always_success_cell(capacity, Some(state_validator_script()));
    let global_state = chain.local_state.last_global_state();
    let initial_rollup_cell_data = global_state.as_bytes();
    let tx = build_simple_tx(
        &mut ctx.inner,
        (spend_cell, Default::default()),
        (rollup_cell.clone(), initial_rollup_cell_data.clone()),
    )
    .as_advanced_builder()
    .cell_dep(ctx.always_success_dep.clone())
    .cell_dep(ctx.state_validator_dep.clone())
    .cell_dep(ctx.rollup_config_dep.clone())
    .build();
    ctx.verify_tx(tx).expect("return success");
    // submit a new block
    let mem_pool_package = {
        let param = PackageParam {
            deposition_requests: Vec::new(),
            max_withdrawal_capacity: std::u128::MAX,
        };
        chain.mem_pool.lock().package(param).unwrap()
    };
    let param = ProduceBlockParam {
        block_producer_id: 0,
    };
    let block_result = chain.produce_block(param, mem_pool_package).unwrap();
    // verify submit block
    let rollup_cell_data = block_result.global_state.as_bytes();
    let witness = {
        let rollup_action = RollupAction::new_builder()
            .set(RollupActionUnion::RollupSubmitBlock(
                RollupSubmitBlock::new_builder()
                    .block(block_result.block.clone())
                    .build(),
            ))
            .build();
        ckb_types::packed::WitnessArgs::new_builder()
            .output_type(CKBPack::pack(&Some(rollup_action.as_bytes())))
            .build()
    };
    let tx = build_simple_tx(
        &mut ctx.inner,
        (rollup_cell.clone(), initial_rollup_cell_data),
        (rollup_cell, rollup_cell_data),
    )
    .as_advanced_builder()
    .input(input_stake_cell)
    .output(output_stake_cell)
    .output_data(CKBPack::pack(&Bytes::default()))
    .cell_dep(ctx.stake_lock_dep.clone())
    .cell_dep(ctx.always_success_dep.clone())
    .cell_dep(ctx.state_validator_dep.clone())
    .cell_dep(ctx.rollup_config_dep.clone())
    .witness(CKBPack::pack(&witness.as_bytes()))
    .build();
    ctx.verify_tx(tx).expect("return success");
}
