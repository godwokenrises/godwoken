use crate::script_tests::programs::CUSTODIAN_LOCK_PROGRAM;
use crate::script_tests::utils::init_env_log;
use crate::script_tests::utils::layer1::{build_simple_tx_with_out_point, random_out_point};
use crate::script_tests::utils::rollup::{random_always_success_script, CellContext};
use crate::script_tests::utils::rollup_config::default_rollup_config;
use ckb_error::assert_error_eq;
use ckb_script::ScriptError;
use ckb_types::{
    core::ScriptHashType,
    packed::{CellDep, CellInput, CellOutput, OutPoint, Script},
};
use gw_types::core::Timepoint;
use gw_types::packed::{BlockMerkleState, CustodianLockArgs, GlobalState};
use gw_types::prelude::*;
use rand::random;

const INDEX_OUT_OF_BOUND_EXIT_CODE: i8 = 1;

#[derive(Debug)]
struct CaseParam {
    // test case id
    id: usize,
    // GlobalState.block.count - 1 - rollup_config.finality_blocks
    finalized_block_number: u64,
    // GlobalState.last_finalized_timepoint
    finalized_block_timestamp: u64,
    // CustodianLockArgs.deposit_block_timepoint
    deposit_block_timepoint: Timepoint,
    // expected running result of the test case, Ok(()) or Err(exit_code)
    expected_result: Result<(), i8>,
}

#[test]
fn test_finality_of_custodian_lock() {
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
            // custodian_state_cell is finalized by block number
            id: 0,
            finalized_block_number,
            finalized_block_timestamp,
            deposit_block_timepoint: finalized_timepoint_by_block_number,
            expected_result: Ok(()),
        },
        CaseParam {
            // custodian_state_cell is not finalized by block number
            id: 1,
            finalized_block_number,
            finalized_block_timestamp,
            deposit_block_timepoint: unfinalized_timepoint_by_block_number,
            expected_result: Err(INDEX_OUT_OF_BOUND_EXIT_CODE),
        },
        CaseParam {
            // custodian_state_cell is finalized by block timestamp
            id: 2,
            finalized_block_number,
            finalized_block_timestamp,
            deposit_block_timepoint: finalized_timepoint_by_block_timestamp,
            expected_result: Ok(()),
        },
        CaseParam {
            // custodian_state_cell is not finalized by block timestamp
            id: 3,
            finalized_block_number,
            finalized_block_timestamp,
            deposit_block_timepoint: unfinalized_timepoint_by_block_timestamp,
            expected_result: Err(INDEX_OUT_OF_BOUND_EXIT_CODE),
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
        deposit_block_timepoint,
        expected_result,
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
    let (mut ctx, rollup_state_out_point, custodian_code_out_point) =
        deploy_context(&prev_global_state);

    // Build custodian_state_cell
    let rollup_state_type_hash = {
        let (rollup_state_cell, _) = ctx.inner.cells.get(&rollup_state_out_point).unwrap();
        let rollup_state_type_script = rollup_state_cell.type_().to_opt().unwrap();
        rollup_state_type_script.calc_script_hash()
    };
    let custodian_code_type_hash = {
        let (custodian_code_cell, _) = ctx.inner.cells.get(&custodian_code_out_point).unwrap();
        let custodian_code_type_script = custodian_code_cell.type_().to_opt().unwrap();
        custodian_code_type_script.calc_script_hash()
    };
    let custodian_state_out_point = random_out_point();
    let custodian_state_cell = CellOutput::new_builder()
        .lock(
            Script::new_builder()
                .code_hash(custodian_code_type_hash.clone())
                .hash_type(ScriptHashType::Type.into())
                .args({
                    let custodian_lock_args = CustodianLockArgs::new_builder()
                        .deposit_block_timepoint(deposit_block_timepoint.full_value().pack())
                        .build();
                    let mut args = Vec::new();
                    args.extend_from_slice(rollup_state_type_hash.as_slice());
                    args.extend_from_slice(custodian_lock_args.as_slice());
                    ckb_types::prelude::Pack::pack(args.as_slice())
                })
                .build(),
        )
        .build();

    // Build transaction
    let tx = build_simple_tx_with_out_point(
        &mut ctx.inner,
        (custodian_state_cell, Default::default()),
        custodian_state_out_point,
        (CellOutput::new_builder().build(), Default::default()),
    )
    .as_advanced_builder()
    // custodian-lock requires rollup_state_cell to be tx input
    .input(CellInput::new(rollup_state_out_point, 0))
    .cell_dep(ctx.rollup_config_dep.clone())
    .cell_dep(ctx.always_success_dep.clone())
    .cell_dep(
        CellDep::new_builder()
            .out_point(custodian_code_out_point)
            .build(),
    )
    .build();
    let actual_result = ctx.verify_tx(tx).map(|_| ());
    let expected_result: Result<_, ckb_error::Error> = expected_result.map_err(|exit_code| {
        ScriptError::ValidationFailure(
            format!("by-type-hash/{:x}", custodian_code_type_hash),
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

// Build common-used cells for testing custodian-lock:
//   - rollup_config_cell, finality_blocks = ROLLUP_CONFIG_FINALITY_BLOCKS
//   - rollup_code_cell, is ALWAYS_SUCCESS_PROGRAM, we won't use it
//   - rollup_state_cell, last_finalized_block_number = ROLLUP_STATE_LAST_FINALIZED_BLOCK_NUMBER
//   - custodian_code_cell, is CUSTODIAN_LOCK_PROGRAM
//
// Return (ctx, rollup_state_out_point, custodian_code_out_point);
fn deploy_context(global_state: &GlobalState) -> (CellContext, OutPoint, OutPoint) {
    let mut ctx = CellContext::new(&default_rollup_config(), Default::default());

    // Build a always-success rollup_state_cell, because we are testing
    // custodian-lock only
    let rollup_state_out_point = deploy_always_success_rollup_state_cell(&mut ctx, global_state);
    let custodian_code_out_point = deploy_custodian_code_cell(&mut ctx);
    (ctx, rollup_state_out_point, custodian_code_out_point)
}

fn deploy_always_success_rollup_state_cell(
    ctx: &mut CellContext,
    global_state: &GlobalState,
) -> OutPoint {
    let rollup_state_data = global_state.as_bytes();
    let rollup_state_cell = CellOutput::new_builder()
        .lock(random_always_success_script())
        .type_(ckb_types::prelude::Pack::pack(&Some(
            random_always_success_script(),
        )))
        .build();
    ctx.insert_cell(rollup_state_cell, rollup_state_data)
}

fn deploy_custodian_code_cell(ctx: &mut CellContext) -> OutPoint {
    let custodian_code_data = CUSTODIAN_LOCK_PROGRAM.clone();
    let custodian_code_cell = CellOutput::new_builder()
        .lock(random_always_success_script())
        .type_(ckb_types::prelude::Pack::pack(&Some(
            random_always_success_script(),
        )))
        .build();
    ctx.insert_cell(custodian_code_cell, custodian_code_data)
}
