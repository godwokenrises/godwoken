use super::*;
use crate::testing_tool::chain::setup_chain;
use crate::{script_tests::utils::layer1::build_simple_tx, testing_tool::chain::construct_block};
use ckb_types::{
    packed::CellInput,
    prelude::{Pack as CKBPack, Unpack},
};
use gw_types::{
    bytes::Bytes,
    core::ScriptHashType,
    packed::{
        CustodianLockArgs, DepositionLockArgs, RollupAction, RollupActionUnion, RollupConfig,
        RollupSubmitBlock, Script, StakeLockArgs, WithdrawalLockArgs,
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
        let cell = build_rollup_locked_cell(
            &rollup_type_script.hash(),
            &stake_script_type_hash,
            stake_capacity,
            StakeLockArgs::default().as_bytes(),
        );
        let out_point = ctx.insert_cell(cell, Bytes::default());
        CellInput::new_builder().previous_output(out_point).build()
    };
    let output_stake_cell = {
        let lock_args = StakeLockArgs::new_builder()
            .stake_block_number(Pack::pack(&1))
            .build();
        build_rollup_locked_cell(
            &rollup_type_script.hash(),
            &stake_script_type_hash,
            stake_capacity,
            lock_args.as_bytes(),
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
    let block_result = {
        let mem_pool = chain.mem_pool.lock();
        construct_block(&chain, &mem_pool, Vec::default()).unwrap()
    };
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

#[test]
fn test_check_reverted_cells_in_submit_block() {
    let rollup_type_script = {
        Script::new_builder()
            .code_hash(Pack::pack(&*STATE_VALIDATOR_CODE_HASH))
            .hash_type(ScriptHashType::Data.into())
            .build()
    };
    // rollup lock & config
    let stake_lock_type = build_type_id_script(b"stake_lock_type_id");
    let stake_script_type_hash: [u8; 32] = stake_lock_type.calc_script_hash().unpack();
    let deposit_lock_type = build_type_id_script(b"deposit_lock_type_id");
    let deposit_script_type_hash: [u8; 32] = deposit_lock_type.calc_script_hash().unpack();
    let custodian_lock_type = build_type_id_script(b"custodian_lock_type_id");
    let custodian_script_type_hash: [u8; 32] = custodian_lock_type.calc_script_hash().unpack();
    let withdrawal_lock_type = build_type_id_script(b"withdrawal_lock_type_id");
    let withdrawal_script_type_hash: [u8; 32] = withdrawal_lock_type.calc_script_hash().unpack();
    let rollup_config = RollupConfig::new_builder()
        .stake_script_type_hash(Pack::pack(&stake_script_type_hash))
        .deposition_script_type_hash(Pack::pack(&deposit_script_type_hash))
        .custodian_script_type_hash(Pack::pack(&custodian_script_type_hash))
        .withdrawal_script_type_hash(Pack::pack(&withdrawal_script_type_hash))
        .build();
    // setup chain
    let chain = setup_chain(rollup_type_script.clone(), rollup_config.clone());
    // deploy scripts
    let param = CellContextParam {
        stake_lock_type: stake_lock_type.clone(),
        deposit_lock_type: deposit_lock_type.clone(),
        custodian_lock_type: custodian_lock_type.clone(),
        withdrawal_lock_type: withdrawal_lock_type.clone(),
        ..Default::default()
    };
    let mut ctx = CellContext::new(&rollup_config, param);
    let stake_capacity = 10000_00000000u64;
    let input_stake_cell = {
        let cell = build_rollup_locked_cell(
            &rollup_type_script.hash(),
            &stake_script_type_hash,
            stake_capacity,
            StakeLockArgs::default().as_bytes(),
        );
        let out_point = ctx.insert_cell(cell, Bytes::default());
        CellInput::new_builder().previous_output(out_point).build()
    };
    let output_stake_cell = {
        let lock_args = StakeLockArgs::new_builder()
            .stake_block_number(Pack::pack(&1))
            .build();
        build_rollup_locked_cell(
            &rollup_type_script.hash(),
            &stake_script_type_hash,
            stake_capacity,
            lock_args.as_bytes(),
        )
    };
    // create a rollup cell
    let capacity = 1000_00000000u64;
    let rollup_cell = build_always_success_cell(capacity, Some(state_validator_script()));
    let global_state = chain.local_state.last_global_state();
    let initial_rollup_cell_data = global_state.as_bytes();
    // build reverted cells inputs and outputs
    let reverted_deposit_capacity: u64 = 200_00000000u64;
    let depositer_lock_script = Script::new_builder()
        .code_hash(Pack::pack(&ALWAYS_SUCCESS_CODE_HASH.clone()))
        .hash_type(ScriptHashType::Data.into())
        .args(Pack::pack(&Bytes::from(b"sender".to_vec())))
        .build();
    let deposit_args = DepositionLockArgs::new_builder()
        .owner_lock_hash(Pack::pack(&[0u8; 32]))
        .layer2_lock(depositer_lock_script.clone())
        .cancel_timeout(Pack::pack(&0))
        .build();
    let revert_block_hash = [42u8; 32];
    let revert_block_number = 2u64;
    // build reverted deposit cell
    let input_reverted_custodian_cell = {
        let args = CustodianLockArgs::new_builder()
            .deposition_lock_args(deposit_args.clone())
            .deposition_block_hash(Pack::pack(&revert_block_hash))
            .deposition_block_number(Pack::pack(&revert_block_number))
            .build();
        let cell = build_rollup_locked_cell(
            &rollup_type_script.hash().into(),
            &custodian_script_type_hash,
            reverted_deposit_capacity,
            args.as_bytes(),
        );
        let out_point = ctx.insert_cell(cell, Bytes::new());
        CellInput::new_builder().previous_output(out_point).build()
    };
    let output_reverted_deposit_cell = {
        build_rollup_locked_cell(
            &rollup_type_script.hash().into(),
            &deposit_script_type_hash,
            reverted_deposit_capacity,
            deposit_args.as_bytes(),
        )
    };
    // build reverted withdrawal cell
    let reverted_withdrawal_capacity: u64 = 130_00000000u64;
    let input_reverted_withdrawal_cell = {
        let args = WithdrawalLockArgs::new_builder()
            .withdrawal_block_hash(Pack::pack(&revert_block_hash))
            .withdrawal_block_number(Pack::pack(&revert_block_number))
            .build();
        let cell = build_rollup_locked_cell(
            &rollup_type_script.hash().into(),
            &withdrawal_script_type_hash,
            reverted_withdrawal_capacity,
            args.as_bytes(),
        );
        let out_point = ctx.insert_cell(cell, Bytes::new());
        CellInput::new_builder().previous_output(out_point).build()
    };
    let output_reverted_custodian_cell = {
        let args = CustodianLockArgs::new_builder()
            .deposition_block_hash(Pack::pack(&[0u8; 32]))
            .deposition_block_number(Pack::pack(&0))
            .build();
        build_rollup_locked_cell(
            &rollup_type_script.hash().into(),
            &custodian_script_type_hash,
            reverted_withdrawal_capacity,
            args.as_bytes(),
        )
    };
    // build arbitrary inputs & outputs finalized custodian cell
    // simulate merge & split finalized custodian cells
    let input_finalized_cells: Vec<_> = {
        let capacity = 300_00000000u64;
        (0..3)
            .into_iter()
            .map(|_| {
                let args = CustodianLockArgs::new_builder()
                    .deposition_block_hash(Pack::pack(&[0u8; 32]))
                    .deposition_block_number(Pack::pack(&0))
                    .build();
                let cell = build_rollup_locked_cell(
                    &rollup_type_script.hash().into(),
                    &custodian_script_type_hash,
                    capacity,
                    args.as_bytes(),
                );
                let out_point = ctx.insert_cell(cell, Bytes::new());
                CellInput::new_builder().previous_output(out_point).build()
            })
            .collect()
    };
    let output_finalized_cells: Vec<_> = {
        let capacity = 450_00000000u64;
        (0..2)
            .into_iter()
            .map(|_| {
                let args = CustodianLockArgs::new_builder()
                    .deposition_block_hash(Pack::pack(&[0u8; 32]))
                    .deposition_block_number(Pack::pack(&0))
                    .build();
                build_rollup_locked_cell(
                    &rollup_type_script.hash().into(),
                    &custodian_script_type_hash,
                    capacity,
                    args.as_bytes(),
                )
            })
            .collect()
    };
    // submit a new block
    let block_result = {
        let mem_pool = chain.mem_pool.lock();
        construct_block(&chain, &mem_pool, Vec::default()).unwrap()
    };
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
    .input(input_reverted_custodian_cell)
    .output(output_reverted_deposit_cell)
    .output_data(CKBPack::pack(&Bytes::default()))
    .input(input_reverted_withdrawal_cell)
    .output(output_reverted_custodian_cell)
    .output_data(CKBPack::pack(&Bytes::default()))
    .inputs(input_finalized_cells)
    .outputs(output_finalized_cells.clone())
    .outputs_data(
        (0..output_finalized_cells.len())
            .into_iter()
            .map(|_| CKBPack::pack(&Bytes::new())),
    )
    .cell_dep(ctx.stake_lock_dep.clone())
    .cell_dep(ctx.deposit_lock_dep.clone())
    .cell_dep(ctx.custodian_lock_dep.clone())
    .cell_dep(ctx.withdrawal_lock_dep.clone())
    .cell_dep(ctx.always_success_dep.clone())
    .cell_dep(ctx.state_validator_dep.clone())
    .cell_dep(ctx.rollup_config_dep.clone())
    .witness(CKBPack::pack(&witness.as_bytes()))
    .build();
    ctx.verify_tx(tx).expect("return success");
}
