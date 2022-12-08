use crate::script_tests::programs::WITHDRAWAL_LOCK_PROGRAM;
use crate::script_tests::utils::init_env_log;
use crate::script_tests::utils::layer1::build_simple_tx_with_out_point;
use crate::script_tests::utils::layer1::random_out_point;
use crate::script_tests::utils::rollup::{random_always_success_script, CellContext};
use crate::script_tests::utils::rollup_config::default_rollup_config;

use ckb_error::assert_error_eq;
use ckb_script::ScriptError;
use ckb_types::prelude::{Builder, Entity};
use gw_types::core::{ScriptHashType, Timepoint};
use gw_types::packed::{
    BlockMerkleState, Byte32, CellDep, CellInput, CellOutput, GlobalState, OutPoint, Script,
    WithdrawalLockArgs,
};
use gw_types::prelude::{Pack, Unpack};
use rand::random;

use super::witness_unlock_withdrawal_via_finalize;
use super::{ToCKBType, ToGWType};

const OWNER_CELL_NOT_FOUND_EXIT_CODE: i8 = 8;
const NOT_FINALIZED_EXIT_CODE: i8 = 45;

#[derive(Debug)]
struct CaseParam {
    // test case id
    id: usize,
    // GlobalState.block.count - 1 - rollup_config.finality_blocks
    finalized_block_number: u64,
    // GlobalState.last_finalized_timepoint
    finalized_block_timestamp: u64,
    // WithdrawalLockArgs.withdrawal_block_timepoint
    withdrawal_block_timepoint: Timepoint,

    // Even if the withdrawal cell is finalized, it needs one of the below condition matched:
    // - the same-index output with withdrawal cell has the same content
    // - transaction inputs includes the one of withdrawal owner inputs
    unlock_path_same_index_same_content_output: bool,
    unlock_path_include_one_owner_input: bool,

    // expected running result of the test case, Ok(()) or Err(exit_code)
    expected_result: Result<(), i8>,
}

#[test]
fn test_finality_of_withdrawal_lock() {
    init_env_log();
    let finalized_block_number = random::<u32>() as u64 + 100;
    let finalized_block_timestamp = random::<u32>() as u64 + 7 * 24 * 60 * 60 * 1000;
    let finalized_timepoint_by_block_number = Timepoint::from_block_number(finalized_block_number);
    let finalized_timepoint_by_block_timestamp =
        Timepoint::from_timestamp(finalized_block_timestamp);
    let unfinalized_timepoint_by_block_number =
        Timepoint::from_block_number(finalized_block_number + 1);
    let unfinalized_timepoint_by_block_timestamp =
        Timepoint::from_timestamp(finalized_block_timestamp + 1);

    let cases = vec![
        CaseParam {
            // withdrawal_state_cell is finalized by block number
            id: 0,
            finalized_block_number,
            finalized_block_timestamp,
            withdrawal_block_timepoint: finalized_timepoint_by_block_number.clone(),
            unlock_path_include_one_owner_input: true,
            unlock_path_same_index_same_content_output: false,
            expected_result: Ok(()),
        },
        CaseParam {
            // withdrawal_state_cell is not finalized by block number
            id: 1,
            finalized_block_number,
            finalized_block_timestamp,
            withdrawal_block_timepoint: unfinalized_timepoint_by_block_number,
            unlock_path_include_one_owner_input: true,
            unlock_path_same_index_same_content_output: false,
            expected_result: Err(NOT_FINALIZED_EXIT_CODE),
        },
        CaseParam {
            // withdrawal_state_cell is finalized by block timestamp
            id: 2,
            finalized_block_number,
            finalized_block_timestamp,
            withdrawal_block_timepoint: finalized_timepoint_by_block_timestamp,
            unlock_path_include_one_owner_input: true,
            unlock_path_same_index_same_content_output: false,
            expected_result: Ok(()),
        },
        CaseParam {
            // withdrawal_state_cell is not finalized by block timestamp
            id: 3,
            finalized_block_number,
            finalized_block_timestamp,
            withdrawal_block_timepoint: unfinalized_timepoint_by_block_timestamp,
            unlock_path_include_one_owner_input: true,
            unlock_path_same_index_same_content_output: false,
            expected_result: Err(NOT_FINALIZED_EXIT_CODE),
        },
        CaseParam {
            // withdrawal_state_cell is finalized by block number, but we miss the owner input
            id: 4,
            finalized_block_number,
            finalized_block_timestamp,
            withdrawal_block_timepoint: finalized_timepoint_by_block_number.clone(),
            unlock_path_include_one_owner_input: false,
            unlock_path_same_index_same_content_output: false,
            expected_result: Err(OWNER_CELL_NOT_FOUND_EXIT_CODE),
        },
        CaseParam {
            // withdrawal_state_cell is finalized by block number, unlock via check_output_cell_has_same_content
            id: 5,
            finalized_block_number,
            finalized_block_timestamp,
            withdrawal_block_timepoint: finalized_timepoint_by_block_number,
            unlock_path_include_one_owner_input: false,
            unlock_path_same_index_same_content_output: true,
            expected_result: Ok(()),
        },
    ];
    cases.into_iter().for_each(run_case);
}

fn run_case(case: CaseParam) {
    println!("{:?}", case);
    let CaseParam {
        id: _id,
        finalized_block_number,
        finalized_block_timestamp,
        withdrawal_block_timepoint,
        expected_result,
        unlock_path_same_index_same_content_output,
        unlock_path_include_one_owner_input,
    } = case;
    let rollup_config = default_rollup_config();

    let rollup_config_hash = rollup_config.hash();
    let global_state_block_count =
        1 + finalized_block_number + rollup_config.finality_blocks().unpack();
    let global_state_last_finalized_timepoint =
        Timepoint::from_timestamp(finalized_block_timestamp);

    // Prepare context
    let prev_global_state = GlobalState::new_builder()
        .rollup_config_hash(rollup_config_hash.pack())
        .block(
            BlockMerkleState::new_builder()
                .count(global_state_block_count.pack())
                .build(),
        )
        .last_finalized_block_number(global_state_last_finalized_timepoint.full_value().pack())
        .build();
    let (mut ctx, rollup_state_out_point, withdrawal_code_out_point, withdrawal_owner_out_point) =
        deploy_context(&prev_global_state);

    let rollup_state_type_hash = {
        let (rollup_state_cell, _) = ctx
            .inner
            .cells
            .get(&rollup_state_out_point.to_ckb())
            .unwrap();
        let rollup_state_type_script = rollup_state_cell.type_().to_opt().unwrap();
        rollup_state_type_script.calc_script_hash()
    };
    let withdrawal_code_type_hash = {
        let (withdrawal_code_cell, _) = ctx
            .inner
            .cells
            .get(&withdrawal_code_out_point.to_ckb())
            .unwrap();
        let withdrawal_code_type_script = withdrawal_code_cell.type_().to_opt().unwrap();
        withdrawal_code_type_script.calc_script_hash()
    };
    let withdrawal_owner_lock_script = {
        let (withdrawal_owner_cell, _) = ctx
            .inner
            .cells
            .get(&withdrawal_owner_out_point.to_ckb())
            .unwrap();
        withdrawal_owner_cell.lock()
    };

    // Build withdrawal_state_cell
    let withdrawal_state_out_point = random_out_point();
    let withdrawal_state_cell = CellOutput::new_builder()
        .lock(
            Script::new_builder()
                .code_hash(withdrawal_code_type_hash.to_gw())
                .hash_type(ScriptHashType::Type.into())
                .args({
                    let withdrawal_owner_lock_hash =
                        withdrawal_owner_lock_script.calc_script_hash();
                    let withdrawal_lock_args = WithdrawalLockArgs::new_builder()
                        .owner_lock_hash(Byte32::new_unchecked(
                            withdrawal_owner_lock_hash.as_bytes(),
                        ))
                        .withdrawal_block_timepoint(withdrawal_block_timepoint.full_value().pack())
                        .build();
                    let mut args = Vec::new();
                    args.extend_from_slice(rollup_state_type_hash.as_slice());
                    args.extend_from_slice(withdrawal_lock_args.as_slice());
                    args.extend_from_slice(
                        (withdrawal_owner_lock_script.as_slice().len() as u32)
                            .to_be_bytes()
                            .as_slice(),
                    );
                    args.extend_from_slice(withdrawal_owner_lock_script.as_slice());
                    args.pack()
                })
                .build(),
        )
        .build();
    let witness_args = witness_unlock_withdrawal_via_finalize();

    // Build transaction
    let mut tx = build_simple_tx_with_out_point(
        &mut ctx.inner,
        (withdrawal_state_cell.to_ckb(), Default::default()),
        withdrawal_state_out_point,
        (
            CellOutput::new_builder().build().to_ckb(),
            Default::default(),
        ),
    )
    .as_advanced_builder()
    .cell_dep(ctx.rollup_config_dep.clone())
    .cell_dep(ctx.always_success_dep.clone())
    .cell_dep(
        CellDep::new_builder()
            .out_point(withdrawal_code_out_point)
            .build()
            .to_ckb(),
    )
    .cell_dep(
        CellDep::new_builder()
            .out_point(rollup_state_out_point)
            .build()
            .to_ckb(),
    )
    .witness(witness_args.as_bytes().to_ckb())
    .build();

    if unlock_path_include_one_owner_input {
        tx = tx
            .as_advanced_builder()
            .input(
                CellInput::new_builder()
                    .previous_output(withdrawal_owner_out_point)
                    .build()
                    .to_ckb(),
            )
            .build();
    }
    if unlock_path_same_index_same_content_output {
        // replace the tx.outputs
        tx = tx
            .as_advanced_builder()
            .set_outputs(vec![CellOutput::new_builder()
                .capacity(withdrawal_state_cell.capacity())
                .type_(withdrawal_state_cell.type_())
                .lock(withdrawal_owner_lock_script.to_gw())
                .build()
                .to_ckb()])
            .set_outputs_data(vec![Default::default()])
            .build();
    }

    let actual_result = ctx.verify_tx(tx).map(|_| ());
    let expected_result: Result<_, ckb_error::Error> = expected_result.map_err(|exit_code| {
        ScriptError::ValidationFailure(
            format!("by-type-hash/{:x}", withdrawal_code_type_hash),
            exit_code,
        )
        .input_lock_script(0)
        .into()
    });

    match (expected_result, actual_result) {
        (Ok(_), Ok(_)) => {}
        (Err(expected_err), Err(actual_err)) => {
            assert_error_eq!(expected_err, actual_err)
        }
        (left, right) => {
            panic!(
                "assertion failed: `(left == right)`\n  left: {:?},\n right: {:?}",
                left, right
            )
        }
    }
}

// Build common-used cells for testing stake-lock:
//   - rollup_config_cell, finality_blocks = ROLLUP_CONFIG_FINALITY_BLOCKS
//   - rollup_code_cell, is ALWAYS_SUCCESS_PROGRAM
//   - rollup_state_cell, last_finalized_block_number = ROLLUP_STATE_LAST_FINALIZED_BLOCK_NUMBER
//   - withdrawal_code_cell, is withdrawal_LOCK_PROGRAM
//   - withdrawal_owner_cell, is ALWAYS_SUCCESS_PROGRAM
//
// Return (ctx, rollup_state_out_point, withdrawal_code_out_point, withdrawal_owner_out_point);
fn deploy_context(global_state: &GlobalState) -> (CellContext, OutPoint, OutPoint, OutPoint) {
    let mut ctx = CellContext::new(&default_rollup_config(), Default::default());

    // Build a always-success rollup_state_cell, because we are testing
    // stake-lock only;
    // Build a stake owner cell, lock script hash is StakeLockArgs.owner_lock_hash
    let rollup_state_out_point = deploy_always_success_rollup_state_cell(&mut ctx, global_state);
    let withdrawal_code_out_point = deploy_withdrawal_code_cell(&mut ctx);
    let withdrawal_owner_out_point = deploy_withdrawal_owner_cell(&mut ctx);
    (
        ctx,
        rollup_state_out_point,
        withdrawal_code_out_point,
        withdrawal_owner_out_point,
    )
}

fn deploy_withdrawal_owner_cell(ctx: &mut CellContext) -> OutPoint {
    let withdrawal_owner_cell = CellOutput::new_builder()
        .lock(random_always_success_script().to_gw())
        .build();
    ctx.insert_cell(withdrawal_owner_cell.to_ckb(), Default::default())
        .to_gw()
}

fn deploy_always_success_rollup_state_cell(
    ctx: &mut CellContext,
    global_state: &GlobalState,
) -> OutPoint {
    let rollup_state_data = global_state.as_bytes();
    let rollup_state_cell = CellOutput::new_builder()
        .lock(random_always_success_script().to_gw())
        .type_(Some(random_always_success_script().to_gw()).pack())
        .build();
    ctx.insert_cell(rollup_state_cell.to_ckb(), rollup_state_data)
        .to_gw()
}

fn deploy_withdrawal_code_cell(ctx: &mut CellContext) -> OutPoint {
    let withdrawal_code_data = WITHDRAWAL_LOCK_PROGRAM.clone();
    let withdrawal_code_cell = CellOutput::new_builder()
        .lock(random_always_success_script().to_gw())
        .type_(Some(random_always_success_script().to_gw()).pack())
        .build();
    ctx.insert_cell(withdrawal_code_cell.to_ckb(), withdrawal_code_data)
        .to_gw()
}
