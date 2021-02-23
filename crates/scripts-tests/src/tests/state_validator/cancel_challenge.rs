use super::*;
use crate::tests::utils::layer1::build_simple_tx;
use ckb_types::{
    packed::CellInput,
    prelude::{Pack as CKBPack, Unpack},
};
use gw_chain::testing_tools::{
    apply_block_result, setup_chain, setup_chain_with_account_lock_manage, ALWAYS_SUCCESS_CODE_HASH,
};
use gw_chain::{
    chain::ProduceBlockParam, mem_pool::PackageParam, next_block_context::NextBlockContext,
};
use gw_common::{h256_ext::H256Ext, sparse_merkle_tree::default_store::DefaultStore, H256};
use gw_generator::account_lock_manage::{always_success::AlwaysSuccess, AccountLockManage};
use gw_types::{
    bytes::Bytes,
    core::{ChallengeTargetType, ScriptHashType, Status},
    packed::{
        Byte32, ChallengeLockArgs, ChallengeTarget, DepositionRequest, RawWithdrawalRequest,
        RollupAction, RollupActionUnion, RollupCancelChallenge, RollupConfig, Script,
        VerifyWithdrawalWitness, WithdrawalRequest,
    },
};

#[test]
fn test_cancel_challenge_via_withdrawal() {
    let rollup_type_script = {
        Script::new_builder()
            .code_hash(Pack::pack(&*STATE_VALIDATOR_CODE_HASH))
            .hash_type(ScriptHashType::Data.into())
            .build()
    };
    // rollup lock & config
    let stake_lock_type = build_type_id_script(b"stake_lock_type_id");
    let challenge_lock_type = build_type_id_script(b"challenge_lock_type_id");
    let eoa_lock_type = build_type_id_script(b"eoa_lock_type_id");
    let challenge_script_type_hash: [u8; 32] = challenge_lock_type.calc_script_hash().unpack();
    let eoa_lock_type_hash: [u8; 32] = eoa_lock_type.calc_script_hash().unpack();
    let allowed_eoa_type_hashes: Vec<Byte32> = vec![Pack::pack(&eoa_lock_type_hash)];
    let rollup_config = RollupConfig::new_builder()
        .challenge_script_type_hash(Pack::pack(&challenge_script_type_hash))
        .allowed_eoa_type_hashes(PackVec::pack(allowed_eoa_type_hashes))
        .build();
    // setup chain
    let mut account_lock_manage = AccountLockManage::default();
    account_lock_manage.register_lock_algorithm(eoa_lock_type_hash.into(), Box::new(AlwaysSuccess));
    let mut chain = setup_chain_with_account_lock_manage(
        rollup_type_script.clone(),
        rollup_config.clone(),
        account_lock_manage,
    );
    // create a rollup cell
    let capacity = 1000_00000000u64;
    let rollup_cell = build_always_success_cell(capacity, Some(state_validator_script()));
    // produce a block so we can challenge it
    let sender_script = {
        // deposit two account
        let sender_script = Script::new_builder()
            .code_hash(Pack::pack(&eoa_lock_type_hash.clone()))
            .hash_type(ScriptHashType::Type.into())
            .args(Pack::pack(&Bytes::from(b"sender".to_vec())))
            .build();
        let receiver_script = Script::new_builder()
            .code_hash(Pack::pack(&ALWAYS_SUCCESS_CODE_HASH.clone()))
            .hash_type(ScriptHashType::Data.into())
            .args(Pack::pack(&Bytes::from(b"receiver".to_vec())))
            .build();
        let deposition_requests = vec![
            DepositionRequest::new_builder()
                .capacity(Pack::pack(&150_00000000u64))
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
        let withdrawal_capacity = 100_00000000u64;
        let withdrawal = WithdrawalRequest::new_builder()
            .raw(
                RawWithdrawalRequest::new_builder()
                    .nonce(Pack::pack(&0u32))
                    .capacity(Pack::pack(&withdrawal_capacity))
                    .account_script_hash(Pack::pack(&sender_script.hash()))
                    .sell_capacity(Pack::pack(&withdrawal_capacity))
                    .build(),
            )
            .build();
        let pkg = {
            let mut mem_pool = chain.mem_pool.lock();
            mem_pool.push_withdrawal_request(withdrawal).unwrap();
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
        sender_script
    };
    // deploy scripts
    let param = CellContextParam {
        stake_lock_type: stake_lock_type.clone(),
        challenge_lock_type: challenge_lock_type.clone(),
        eoa_lock_type: eoa_lock_type.clone(),
        ..Default::default()
    };
    let mut ctx = CellContext::new(&rollup_config, param);
    let challenge_capacity = 10000_00000000u64;
    let challenged_block = chain.local_state.tip().clone();
    let challenge_target_index = 0u32;
    let input_challenge_cell = {
        let lock_args = ChallengeLockArgs::new_builder()
            .target(
                ChallengeTarget::new_builder()
                    .target_index(Pack::pack(&challenge_target_index))
                    .target_type(ChallengeTargetType::Withdrawal.into())
                    .block_hash(Pack::pack(&challenged_block.hash()))
                    .build(),
            )
            .build();
        let cell = build_rollup_locked_cell(
            &rollup_type_script.hash(),
            &challenge_script_type_hash,
            challenge_capacity,
            lock_args.as_bytes(),
        );
        let out_point = ctx.insert_cell(cell, Bytes::new());
        CellInput::new_builder().previous_output(out_point).build()
    };
    let global_state = chain
        .local_state
        .last_global_state()
        .clone()
        .as_builder()
        .status(Status::Halting.into())
        .build();
    let initial_rollup_cell_data = global_state.as_bytes();
    // verify enter challenge
    let witness = {
        let rollup_action = RollupAction::new_builder()
            .set(RollupActionUnion::RollupCancelChallenge(
                RollupCancelChallenge::default(),
            ))
            .build();
        ckb_types::packed::WitnessArgs::new_builder()
            .output_type(CKBPack::pack(&Some(rollup_action.as_bytes())))
            .build()
    };
    let withdrawal = challenged_block
        .withdrawals()
        .get(challenge_target_index as usize)
        .unwrap();
    let challenge_witness = {
        let witness = {
            let withdrawal_proof: Bytes = {
                let mut tree: gw_common::smt::SMT<DefaultStore<H256>> = Default::default();
                for (index, withdrawal) in challenged_block.withdrawals().into_iter().enumerate() {
                    tree.update(
                        H256::from_u32(index as u32),
                        withdrawal.witness_hash().into(),
                    )
                    .unwrap();
                }
                tree.merkle_proof(vec![H256::from_u32(challenge_target_index as u32)])
                    .unwrap()
                    .compile(vec![(
                        H256::from_u32(challenge_target_index as u32),
                        withdrawal.witness_hash().into(),
                    )])
                    .unwrap()
                    .0
                    .into()
            };
            VerifyWithdrawalWitness::new_builder()
                .raw_l2block(challenged_block.raw())
                .account_script(sender_script.clone())
                .withdrawal_request(withdrawal.clone())
                .withdrawal_proof(Pack::pack(&withdrawal_proof))
                .build()
        };
        ckb_types::packed::WitnessArgs::new_builder()
            .lock(CKBPack::pack(&Some(witness.as_bytes())))
            .build()
    };
    let input_unlock_cell = {
        let cell = CellOutput::new_builder()
            .lock(ckb_types::packed::Script::new_unchecked(
                sender_script.as_bytes(),
            ))
            .capacity(CKBPack::pack(&42u64))
            .build();
        let message = {
            let mut hasher = new_blake2b();
            hasher.update(&rollup_type_script.hash());
            hasher.update(withdrawal.raw().as_slice());
            let mut hash = [0u8; 32];
            hasher.finalize(&mut hash);
            hash
        };
        let out_point = ctx.insert_cell(cell, Bytes::from(message.to_vec()));
        CellInput::new_builder().previous_output(out_point).build()
    };
    let rollup_cell_data = global_state
        .clone()
        .as_builder()
        .status(Status::Running.into())
        .build()
        .as_bytes();
    let tx = build_simple_tx(
        &mut ctx.inner,
        (rollup_cell.clone(), initial_rollup_cell_data),
        (rollup_cell, rollup_cell_data),
    )
    .as_advanced_builder()
    .witness(CKBPack::pack(&witness.as_bytes()))
    .input(input_challenge_cell)
    .witness(CKBPack::pack(&challenge_witness.as_bytes()))
    .input(input_unlock_cell)
    .witness(Default::default())
    .cell_dep(ctx.challenge_lock_dep.clone())
    .cell_dep(ctx.stake_lock_dep.clone())
    .cell_dep(ctx.always_success_dep.clone())
    .cell_dep(ctx.state_validator_dep.clone())
    .cell_dep(ctx.rollup_config_dep.clone())
    .cell_dep(ctx.eoa_lock_dep.clone())
    .build();
    ctx.verify_tx(tx).expect("return success");
}
