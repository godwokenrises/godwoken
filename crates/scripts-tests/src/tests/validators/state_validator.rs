use super::{STATE_VALIDATOR_CODE_HASH, STATE_VALIDATOR_PROGRAM};
use crate::tests::utils::layer1::{
    always_success_script, build_resolved_tx, build_simple_tx, random_out_point, DummyDataLoader,
    ALWAYS_SUCCESS_CODE_HASH, ALWAYS_SUCCESS_PROGRAM, MAX_CYCLES,
};
use ckb_script::TransactionScriptsVerifier;
use ckb_types::{
    packed::{CellDep, CellInput, CellOutput},
    prelude::{Pack as CKBPack, Unpack},
};
use gw_chain::testing_tools::{
    apply_block_result, setup_chain, ALWAYS_SUCCESS_ACCOUNT_LOCK_CODE_HASH,
};
use gw_chain::{
    chain::ProduceBlockParam, mem_pool::PackageParam, next_block_context::NextBlockContext,
};
use gw_common::{builtins::CKB_SUDT_ACCOUNT_ID, state::State};
use gw_types::{
    bytes::Bytes,
    core::{ChallengeTargetType, ScriptHashType, Status},
    packed::{
        ChallengeLockArgs, ChallengeTarget, ChallengeWitness, DepositionRequest, L2Transaction,
        RawL2Transaction, RollupAction, RollupActionUnion, RollupConfig, RollupEnterChallenge,
        RollupSubmitBlock, SUDTArgs, SUDTArgsUnion, SUDTTransfer, Script, StakeLockArgs,
    },
    prelude::*,
};

struct CellContext {
    inner: DummyDataLoader,
    state_validator_dep: CellDep,
    rollup_config_dep: CellDep,
    stake_lock_dep: CellDep,
    always_success_dep: CellDep,
}

impl CellContext {
    fn new(rollup_config: &RollupConfig, stake_lock_type: ckb_types::packed::Script) -> Self {
        let mut data_loader = DummyDataLoader::default();
        let always_success_dep = {
            let always_success_out_point = random_out_point();
            data_loader.cells.insert(
                always_success_out_point.clone(),
                (
                    CellOutput::new_builder()
                        .capacity(CKBPack::pack(&(ALWAYS_SUCCESS_PROGRAM.len() as u64)))
                        .build(),
                    ALWAYS_SUCCESS_PROGRAM.clone(),
                ),
            );
            CellDep::new_builder()
                .out_point(always_success_out_point)
                .build()
        };
        let state_validator_dep = {
            let state_validator_out_point = random_out_point();
            data_loader.cells.insert(
                state_validator_out_point.clone(),
                (
                    CellOutput::new_builder()
                        .capacity(CKBPack::pack(&(STATE_VALIDATOR_PROGRAM.len() as u64)))
                        .build(),
                    STATE_VALIDATOR_PROGRAM.clone(),
                ),
            );
            CellDep::new_builder()
                .out_point(state_validator_out_point)
                .build()
        };
        let rollup_config_dep = {
            let rollup_config_out_point = random_out_point();
            data_loader.cells.insert(
                rollup_config_out_point.clone(),
                (
                    CellOutput::new_builder()
                        .capacity(CKBPack::pack(&(rollup_config.as_bytes().len() as u64)))
                        .build(),
                    rollup_config.as_bytes(),
                ),
            );
            CellDep::new_builder()
                .out_point(rollup_config_out_point)
                .build()
        };
        let stake_lock_dep = {
            let stake_out_point = random_out_point();
            data_loader.cells.insert(
                stake_out_point.clone(),
                (
                    CellOutput::new_builder()
                        .capacity(CKBPack::pack(&(ALWAYS_SUCCESS_PROGRAM.len() as u64)))
                        .type_(CKBPack::pack(&Some(stake_lock_type)))
                        .build(),
                    ALWAYS_SUCCESS_PROGRAM.clone(),
                ),
            );
            CellDep::new_builder().out_point(stake_out_point).build()
        };
        CellContext {
            inner: data_loader,
            rollup_config_dep,
            always_success_dep,
            stake_lock_dep,
            state_validator_dep,
        }
    }

    fn insert_cell(
        &mut self,
        cell: ckb_types::packed::CellOutput,
        data: Bytes,
    ) -> ckb_types::packed::OutPoint {
        let out_point = random_out_point();
        self.inner.cells.insert(out_point.clone(), (cell, data));
        out_point
    }

    fn verify_tx(
        &self,
        tx: ckb_types::core::TransactionView,
    ) -> Result<ckb_types::core::Cycle, ckb_error::Error> {
        let resolved_tx = build_resolved_tx(&self.inner, &tx);
        let mut verifier = TransactionScriptsVerifier::new(&resolved_tx, &self.inner);
        verifier.set_debug_printer(|_script, msg| println!("[script debug] {}", msg));
        verifier.verify(MAX_CYCLES)
    }
}

fn state_validator_script() -> ckb_types::packed::Script {
    ckb_types::packed::Script::new_builder()
        .code_hash(CKBPack::pack(&*STATE_VALIDATOR_CODE_HASH))
        .hash_type(ScriptHashType::Data.into())
        .build()
}

fn build_type_id_script(name: &[u8]) -> ckb_types::packed::Script {
    ckb_types::packed::Script::new_builder()
        .code_hash(CKBPack::pack(&ALWAYS_SUCCESS_CODE_HASH.clone()))
        .args(CKBPack::pack(&Bytes::from(name.to_vec())))
        .build()
}

fn build_stake_cell(
    rollup_type_script_hash: &[u8; 32],
    stake_script_type_hash: &[u8; 32],
    stake_capacity: u64,
    lock_args: StakeLockArgs,
) -> ckb_types::packed::CellOutput {
    let stake_lock = {
        let mut args = Vec::new();
        args.extend_from_slice(rollup_type_script_hash);
        args.extend_from_slice(lock_args.as_slice());
        ckb_types::packed::Script::new_builder()
            .code_hash(CKBPack::pack(stake_script_type_hash))
            .hash_type(ScriptHashType::Type.into())
            .args(CKBPack::pack(&Bytes::from(args)))
            .build()
    };
    CellOutput::new_builder()
        .lock(stake_lock)
        .capacity(CKBPack::pack(&stake_capacity))
        .build()
}

fn build_challenge_cell(
    rollup_type_script_hash: &[u8; 32],
    challenge_script_type_hash: &[u8; 32],
    capacity: u64,
    lock_args: ChallengeLockArgs,
) -> ckb_types::packed::CellOutput {
    let lock = {
        let mut args = Vec::new();
        args.extend_from_slice(rollup_type_script_hash);
        args.extend_from_slice(lock_args.as_slice());
        ckb_types::packed::Script::new_builder()
            .code_hash(CKBPack::pack(challenge_script_type_hash))
            .hash_type(ScriptHashType::Type.into())
            .args(CKBPack::pack(&Bytes::from(args)))
            .build()
    };
    CellOutput::new_builder()
        .lock(lock)
        .capacity(CKBPack::pack(&capacity))
        .build()
}

fn build_always_success_cell(
    capacity: u64,
    type_: Option<ckb_types::packed::Script>,
) -> ckb_types::packed::CellOutput {
    CellOutput::new_builder()
        .lock(always_success_script())
        .type_(CKBPack::pack(&type_))
        .capacity(CKBPack::pack(&capacity))
        .build()
}

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
    let mut ctx = CellContext::new(&rollup_config, stake_lock_type.clone());
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

#[test]
fn test_enter_challenge() {
    let rollup_type_script = {
        Script::new_builder()
            .code_hash(Pack::pack(&*STATE_VALIDATOR_CODE_HASH))
            .hash_type(ScriptHashType::Data.into())
            .build()
    };
    // rollup lock & config
    let stake_lock_type = build_type_id_script(b"stake_lock_type_id");
    let stake_script_type_hash: [u8; 32] = stake_lock_type.calc_script_hash().unpack();
    let challenge_lock_type = build_type_id_script(b"challenge_lock_type_id");
    let challenge_script_type_hash: [u8; 32] = challenge_lock_type.calc_script_hash().unpack();
    let rollup_config = RollupConfig::new_builder()
        .stake_script_type_hash(Pack::pack(&stake_script_type_hash))
        .challenge_script_type_hash(Pack::pack(&challenge_script_type_hash))
        .build();
    // setup chain
    let mut chain = setup_chain(rollup_type_script.clone(), rollup_config.clone());
    // create a rollup cell
    let capacity = 1000_00000000u64;
    let rollup_cell = build_always_success_cell(capacity, Some(state_validator_script()));
    // produce a block so we can challenge it
    {
        // deposit two account
        let sender_script = Script::new_builder()
            .code_hash(Pack::pack(&ALWAYS_SUCCESS_ACCOUNT_LOCK_CODE_HASH.clone()))
            .hash_type(ScriptHashType::Data.into())
            .args(Pack::pack(&Bytes::from(b"sender".to_vec())))
            .build();
        let receiver_script = Script::new_builder()
            .code_hash(Pack::pack(&ALWAYS_SUCCESS_ACCOUNT_LOCK_CODE_HASH.clone()))
            .hash_type(ScriptHashType::Data.into())
            .args(Pack::pack(&Bytes::from(b"receiver".to_vec())))
            .build();
        let deposition_requests = vec![
            DepositionRequest::new_builder()
                .capacity(Pack::pack(&100_00000000u64))
                .script(sender_script.clone())
                .build(),
            DepositionRequest::new_builder()
                .capacity(Pack::pack(&50_00000000u64))
                .script(receiver_script.clone())
                .build(),
        ];
        let pkg = {
            let mut mem_pool = chain.mem_pool.lock();
            mem_pool
                .package(PackageParam {
                    deposition_requests: deposition_requests.clone(),
                    max_withdrawal_capacity: std::u128::MAX,
                })
                .unwrap()
        };
        let produce_block_result = chain
            .produce_block(
                ProduceBlockParam {
                    block_producer_id: 0,
                },
                pkg,
            )
            .unwrap();
        let rollup_cell = gw_types::packed::CellOutput::new_unchecked(rollup_cell.as_bytes());
        let nb_ctx = NextBlockContext {
            block_producer_id: 0,
            timestamp: 0,
        };
        apply_block_result(
            &mut chain,
            rollup_cell.clone(),
            nb_ctx.clone(),
            produce_block_result,
            deposition_requests,
        );
        let db = chain.store().begin_transaction();
        let tree = db.account_state_tree().unwrap();
        let sender_id = tree
            .get_account_id_by_script_hash(&sender_script.hash().into())
            .unwrap()
            .unwrap();
        let receiver_id = tree
            .get_account_id_by_script_hash(&receiver_script.hash().into())
            .unwrap()
            .unwrap();
        let pkg = {
            let args = SUDTArgs::new_builder()
                .set(SUDTArgsUnion::SUDTTransfer(
                    SUDTTransfer::new_builder()
                        .amount(Pack::pack(&50_00000000u128))
                        .to(Pack::pack(&receiver_id))
                        .build(),
                ))
                .build()
                .as_bytes();
            let tx = L2Transaction::new_builder()
                .raw(
                    RawL2Transaction::new_builder()
                        .from_id(Pack::pack(&sender_id))
                        .to_id(Pack::pack(&CKB_SUDT_ACCOUNT_ID))
                        .nonce(Pack::pack(&0u32))
                        .args(Pack::pack(&args))
                        .build(),
                )
                .build();
            let mut mem_pool = chain.mem_pool.lock();
            mem_pool.push(tx).unwrap();
            mem_pool
                .package(PackageParam {
                    deposition_requests: vec![],
                    max_withdrawal_capacity: std::u128::MAX,
                })
                .unwrap()
        };
        let produce_block_result = chain
            .produce_block(
                ProduceBlockParam {
                    block_producer_id: 0,
                },
                pkg,
            )
            .unwrap();
        apply_block_result(
            &mut chain,
            rollup_cell,
            nb_ctx,
            produce_block_result,
            vec![],
        );
    }
    // deploy scripts
    let mut ctx = CellContext::new(&rollup_config, stake_lock_type.clone());
    let challenge_capacity = 10000_00000000u64;
    let challenge_cell = {
        let lock_args = ChallengeLockArgs::new_builder()
            .target(
                ChallengeTarget::new_builder()
                    .target_index(Pack::pack(&0u32))
                    .target_type(ChallengeTargetType::Transaction.into())
                    .build(),
            )
            .build();
        build_challenge_cell(
            &rollup_type_script.hash(),
            &challenge_script_type_hash,
            challenge_capacity,
            lock_args,
        )
    };
    let global_state = chain.local_state.last_global_state();
    let initial_rollup_cell_data = global_state.as_bytes();
    // verify enter challenge
    let witness = {
        let challenged_block = chain.local_state.tip().clone();
        let block_proof: Bytes = {
            let db = chain.store().begin_transaction();
            let proof = db
                .block_smt()
                .unwrap()
                .merkle_proof(vec![challenged_block.smt_key().into()])
                .unwrap();
            proof
                .compile(vec![(
                    challenged_block.smt_key().into(),
                    challenged_block.hash().into(),
                )])
                .unwrap()
                .0
                .into()
        };
        let witness = ChallengeWitness::new_builder()
            .raw_l2block(challenged_block.raw())
            .block_proof(Pack::pack(&block_proof))
            .build();
        let rollup_action = RollupAction::new_builder()
            .set(RollupActionUnion::RollupEnterChallenge(
                RollupEnterChallenge::new_builder().witness(witness).build(),
            ))
            .build();
        ckb_types::packed::WitnessArgs::new_builder()
            .output_type(CKBPack::pack(&Some(rollup_action.as_bytes())))
            .build()
    };
    let rollup_cell_data = global_state
        .clone()
        .as_builder()
        .status(Status::Halting.into())
        .build()
        .as_bytes();
    let tx = build_simple_tx(
        &mut ctx.inner,
        (rollup_cell.clone(), initial_rollup_cell_data),
        (rollup_cell, rollup_cell_data),
    )
    .as_advanced_builder()
    .output(challenge_cell)
    .output_data(CKBPack::pack(&Bytes::default()))
    .cell_dep(ctx.stake_lock_dep.clone())
    .cell_dep(ctx.always_success_dep.clone())
    .cell_dep(ctx.state_validator_dep.clone())
    .cell_dep(ctx.rollup_config_dep.clone())
    .witness(CKBPack::pack(&witness.as_bytes()))
    .build();
    ctx.verify_tx(tx).expect("return success");
}
