use super::*;
use crate::script_tests::utils::layer1::build_simple_tx;
use crate::testing_tool::chain::{
    apply_block_result, construct_block, setup_chain, ALWAYS_SUCCESS_CODE_HASH,
};
use ckb_types::{
    packed::CellInput,
    prelude::{Pack as CKBPack, Unpack as CKBUnpack},
};
use gw_common::{
    builtins::CKB_SUDT_ACCOUNT_ID, h256_ext::H256Ext,
    sparse_merkle_tree::default_store::DefaultStore, state::State, H256,
};
use gw_store::state_db::StateDBVersion;
use gw_types::{
    bytes::Bytes,
    core::{ChallengeTargetType, ScriptHashType, Status},
    packed::{
        ChallengeLockArgs, ChallengeTarget, DepositionRequest, L2Transaction, RawL2Transaction,
        RollupAction, RollupActionUnion, RollupConfig, RollupRevert, SUDTArgs, SUDTArgsUnion,
        SUDTTransfer, Script,
    },
};

#[test]
fn test_revert() {
    let rollup_type_script = {
        Script::new_builder()
            .code_hash(Pack::pack(&*STATE_VALIDATOR_CODE_HASH))
            .hash_type(ScriptHashType::Data.into())
            .build()
    };
    // rollup lock & config
    let reward_receive_lock = always_success_script()
        .as_builder()
        .args(CKBPack::pack(&Bytes::from(b"reward_receive_lock".to_vec())))
        .build();
    let reward_burn_lock = ckb_types::packed::Script::new_builder()
        .args(CKBPack::pack(&Bytes::from(b"reward_burned_lock".to_vec())))
        .code_hash(CKBPack::pack(&[0u8; 32]))
        .build();
    let reward_burn_lock_hash: [u8; 32] = reward_burn_lock.calc_script_hash().unpack();
    let stake_lock_type = build_type_id_script(b"stake_lock_type_id");
    let stake_script_type_hash: [u8; 32] = stake_lock_type.calc_script_hash().unpack();
    let challenge_lock_type = build_type_id_script(b"challenge_lock_type_id");
    let challenge_script_type_hash: [u8; 32] = challenge_lock_type.calc_script_hash().unpack();
    let rollup_config = RollupConfig::new_builder()
        .stake_script_type_hash(Pack::pack(&stake_script_type_hash))
        .challenge_script_type_hash(Pack::pack(&challenge_script_type_hash))
        .reward_burn_rate(50u8.into())
        .burn_lock_hash(Pack::pack(&reward_burn_lock_hash))
        .build();
    // setup chain
    let mut chain = setup_chain(rollup_type_script.clone(), rollup_config.clone());
    // create a rollup cell
    let capacity = 1000_00000000u64;
    let rollup_cell = build_always_success_cell(capacity, Some(state_validator_script()));
    // produce a block so we can challenge it
    let prev_block_merkle = {
        // deposit two account
        let sender_script = Script::new_builder()
            .code_hash(Pack::pack(&ALWAYS_SUCCESS_CODE_HASH.clone()))
            .hash_type(ScriptHashType::Data.into())
            .args(Pack::pack(&Bytes::from(b"sender".to_vec())))
            .build();
        let receiver_script = Script::new_builder()
            .code_hash(Pack::pack(&ALWAYS_SUCCESS_CODE_HASH.clone()))
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
        let produce_block_result = {
            let mem_pool = chain.mem_pool.lock();
            construct_block(&chain, &mem_pool, deposition_requests.clone()).unwrap()
        };
        let rollup_cell = gw_types::packed::CellOutput::new_unchecked(rollup_cell.as_bytes());
        apply_block_result(
            &mut chain,
            rollup_cell.clone(),
            produce_block_result,
            deposition_requests,
        );
        let tip_block_hash = chain.store().get_tip_block_hash().unwrap();
        let db = chain
            .store()
            .state_at(StateDBVersion::from_block_hash(tip_block_hash))
            .unwrap();
        let tree = db.account_state_tree().unwrap();
        let sender_id = tree
            .get_account_id_by_script_hash(&sender_script.hash().into())
            .unwrap()
            .unwrap();
        let receiver_id = tree
            .get_account_id_by_script_hash(&receiver_script.hash().into())
            .unwrap()
            .unwrap();
        let produce_block_result = {
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
            mem_pool.push_transaction(tx).unwrap();
            construct_block(&chain, &mem_pool, Vec::default()).unwrap()
        };
        let prev_block_merkle = chain.local_state.last_global_state().block();
        apply_block_result(&mut chain, rollup_cell, produce_block_result, vec![]);
        prev_block_merkle
    };
    // deploy scripts
    let param = CellContextParam {
        stake_lock_type: stake_lock_type.clone(),
        challenge_lock_type: challenge_lock_type.clone(),
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
    let challenge_capacity = 10000_00000000u64;
    let challenged_block = chain.local_state.tip().clone();
    let input_challenge_cell = {
        let lock_args = ChallengeLockArgs::new_builder()
            .target(
                ChallengeTarget::new_builder()
                    .target_index(Pack::pack(&0u32))
                    .target_type(ChallengeTargetType::Transaction.into())
                    .block_hash(Pack::pack(&challenged_block.hash()))
                    .build(),
            )
            .rewards_receiver_lock(gw_types::packed::Script::new_unchecked(
                reward_receive_lock.as_bytes(),
            ))
            .build();
        let cell = build_rollup_locked_cell(
            &rollup_type_script.hash(),
            &challenge_script_type_hash,
            challenge_capacity,
            lock_args.as_bytes(),
        );
        let out_point = ctx.insert_cell(cell, Bytes::new());
        let since: u64 = {
            let mut since = 1 << 63;
            since |= rollup_config.challenge_maturity_blocks().unpack();
            since
        };
        CellInput::new_builder()
            .since(CKBPack::pack(&since))
            .previous_output(out_point)
            .build()
    };
    let burn_rate: u8 = rollup_config.reward_burn_rate().into();
    let reward_capacity: u64 = stake_capacity * burn_rate as u64 / 100;
    let received_capacity: u64 = reward_capacity + challenge_capacity;
    let burned_capacity: u64 = stake_capacity - reward_capacity;
    let receive_cell = CellOutput::new_builder()
        .capacity(CKBPack::pack(&received_capacity))
        .lock(reward_receive_lock)
        .build();
    let reward_burned_cell = CellOutput::new_builder()
        .capacity(CKBPack::pack(&burned_capacity))
        .lock(reward_burn_lock)
        .build();
    let global_state = chain
        .local_state
        .last_global_state()
        .clone()
        .as_builder()
        .status(Status::Halting.into())
        .build();
    let initial_rollup_cell_data = global_state.as_bytes();
    let mut reverted_block_tree: gw_common::smt::SMT<DefaultStore<H256>> = Default::default();
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
        let reverted_block_proof: Bytes = {
            reverted_block_tree
                .merkle_proof(vec![challenged_block.hash().into()])
                .unwrap()
                .compile(vec![(challenged_block.hash().into(), H256::zero())])
                .unwrap()
                .0
                .into()
        };
        let rollup_action = RollupAction::new_builder()
            .set(RollupActionUnion::RollupRevert(
                RollupRevert::new_builder()
                    .reverted_blocks(vec![challenged_block.raw()].pack())
                    .block_proof(Pack::pack(&block_proof))
                    .reverted_block_proof(Pack::pack(&reverted_block_proof))
                    .build(),
            ))
            .build();
        ckb_types::packed::WitnessArgs::new_builder()
            .output_type(CKBPack::pack(&Some(rollup_action.as_bytes())))
            .build()
    };
    let post_reverted_block_root = {
        reverted_block_tree
            .update(challenged_block.hash().into(), H256::one())
            .unwrap();
        reverted_block_tree.root().clone()
    };
    let last_finalized_block_number = {
        let number: u64 = challenged_block.raw().number().unpack();
        let finalize_blocks = rollup_config.finality_blocks().unpack();
        (number - 1).saturating_sub(finalize_blocks)
    };
    let rollup_cell_data = global_state
        .clone()
        .as_builder()
        .status(Status::Running.into())
        .reverted_block_root(Pack::pack(&post_reverted_block_root))
        .last_finalized_block_number(Pack::pack(&last_finalized_block_number))
        .account(challenged_block.raw().prev_account())
        .block(prev_block_merkle)
        .tip_block_hash(challenged_block.raw().parent_block_hash())
        .build()
        .as_bytes();
    let tx = build_simple_tx(
        &mut ctx.inner,
        (rollup_cell.clone(), initial_rollup_cell_data),
        (rollup_cell, rollup_cell_data),
    )
    .as_advanced_builder()
    .input(input_challenge_cell)
    .input(input_stake_cell)
    .output(receive_cell)
    .output_data(Default::default())
    .output(reward_burned_cell)
    .output_data(Default::default())
    .cell_dep(ctx.challenge_lock_dep.clone())
    .cell_dep(ctx.stake_lock_dep.clone())
    .cell_dep(ctx.always_success_dep.clone())
    .cell_dep(ctx.state_validator_dep.clone())
    .cell_dep(ctx.rollup_config_dep.clone())
    .witness(CKBPack::pack(&witness.as_bytes()))
    .witness(CKBPack::pack(&Bytes::new()))
    .build();
    ctx.verify_tx(tx).expect("return success");
}
