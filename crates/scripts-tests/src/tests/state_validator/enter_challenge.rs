use super::*;
use crate::tests::utils::layer1::build_simple_tx;
use ckb_types::prelude::{Pack as CKBPack, Unpack};
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
        SUDTArgs, SUDTArgsUnion, SUDTTransfer, Script,
    },
};

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
    let challenge_lock_type = build_type_id_script(b"challenge_lock_type_id");
    let challenge_script_type_hash: [u8; 32] = challenge_lock_type.calc_script_hash().unpack();
    let rollup_config = RollupConfig::new_builder()
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
    let param = CellContextParam {
        stake_lock_type: stake_lock_type.clone(),
        ..Default::default()
    };
    let mut ctx = CellContext::new(&rollup_config, param);
    let challenged_block = chain.local_state.tip().clone();
    let challenge_capacity = 10000_00000000u64;
    let challenge_cell = {
        let lock_args = ChallengeLockArgs::new_builder()
            .target(
                ChallengeTarget::new_builder()
                    .target_index(Pack::pack(&0u32))
                    .target_type(ChallengeTargetType::Transaction.into())
                    .block_hash(Pack::pack(&challenged_block.hash()))
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
