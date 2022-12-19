#![allow(clippy::mutable_key_type)]

use std::collections::HashSet;

use crate::script_tests::programs::STATE_VALIDATOR_CODE_HASH;
use crate::script_tests::utils::init_env_log;
use crate::script_tests::utils::layer1::build_simple_tx_with_out_point;
use crate::script_tests::utils::layer1::random_out_point;
use crate::script_tests::utils::rollup::{
    build_always_success_cell, build_rollup_locked_cell, calculate_type_id,
    named_always_success_script, CellContext, CellContextParam,
};
use crate::testing_tool::chain::{apply_block_result, construct_block};
use crate::testing_tool::chain::{
    into_deposit_info_cell, setup_chain_with_config, ALWAYS_SUCCESS_CODE_HASH,
};
use ckb_error::assert_error_eq;
use ckb_script::ScriptError;
use ckb_types::packed::CellOutput;
use ckb_types::prelude::{Pack as CKBPack, Unpack};
use gw_chain::chain::Chain;
use gw_common::registry_address::RegistryAddress;
use gw_common::{builtins::CKB_SUDT_ACCOUNT_ID, state::State};
use gw_store::state::history::history_state::RWConfig;
use gw_store::state::BlockStateDB;
use gw_types::core::AllowedContractType;
use gw_types::core::AllowedEoaType;
use gw_types::packed::AllowedTypeHash;
use gw_types::packed::Fee;
use gw_types::prelude::*;
use gw_types::U256;
use gw_types::{
    bytes::Bytes,
    core::{ChallengeTargetType, ScriptHashType, Status},
    packed::{
        ChallengeLockArgs, ChallengeTarget, ChallengeWitness, DepositRequest, L2Transaction,
        RawL2Transaction, RollupAction, RollupActionUnion, RollupConfig, RollupEnterChallenge,
        SUDTArgs, SUDTArgsUnion, SUDTTransfer, Script,
    },
};

const INVALID_CHALLENGE_TARGET_ERROR: i8 = 32;

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_enter_challenge() {
    init_env_log();
    let input_out_point = random_out_point();
    let type_id = calculate_type_id(input_out_point.clone());
    let rollup_type_script = {
        Script::new_builder()
            .code_hash(Pack::pack(&*STATE_VALIDATOR_CODE_HASH))
            .hash_type(ScriptHashType::Data.into())
            .args(Pack::pack(&Bytes::from(type_id.to_vec())))
            .build()
    };
    // rollup lock & config
    let stake_lock_type = named_always_success_script(b"stake_lock_type_id");
    let challenge_lock_type = named_always_success_script(b"challenge_lock_type_id");
    let challenge_script_type_hash: [u8; 32] =
        challenge_lock_type.calc_script_hash().unpack().into();
    let finality_blocks = 10;
    let rollup_config = RollupConfig::new_builder()
        .challenge_script_type_hash(Pack::pack(&challenge_script_type_hash))
        .finality_blocks(Pack::pack(&finality_blocks))
        .allowed_eoa_type_hashes(
            vec![AllowedTypeHash::new(
                AllowedEoaType::Eth,
                *ALWAYS_SUCCESS_CODE_HASH,
            )]
            .pack(),
        )
        .allowed_contract_type_hashes(
            vec![AllowedTypeHash::new(AllowedContractType::Sudt, [0u8; 32])].pack(),
        )
        .build();
    // setup chain
    let mut chain = setup_chain_with_config(rollup_type_script.clone(), rollup_config).await;
    // create a rollup cell
    let capacity = 1000_00000000u64;
    let rollup_cell = build_always_success_cell(
        capacity,
        Some(ckb_types::packed::Script::new_unchecked(
            rollup_type_script.as_bytes(),
        )),
    );
    // produce a block so we can challenge it
    {
        // deposit two account
        let rollup_script_hash = rollup_type_script.hash();
        let mut sender_args = rollup_script_hash.to_vec();
        sender_args.extend_from_slice(&[1u8; 20]);
        let sender_script = Script::new_builder()
            .code_hash(Pack::pack(&ALWAYS_SUCCESS_CODE_HASH.clone()))
            .hash_type(ScriptHashType::Type.into())
            .args(Pack::pack(&Bytes::from(sender_args)))
            .build();
        let mut receiver_args = rollup_script_hash.to_vec();
        receiver_args.extend_from_slice(&[2u8; 20]);
        let receiver_script = Script::new_builder()
            .code_hash(Pack::pack(&ALWAYS_SUCCESS_CODE_HASH.clone()))
            .hash_type(ScriptHashType::Type.into())
            .args(Pack::pack(&Bytes::from(receiver_args)))
            .build();
        let deposit_requests = vec![
            DepositRequest::new_builder()
                .capacity(Pack::pack(&300_00000000u64))
                .script(sender_script.clone())
                .registry_id(Pack::pack(&gw_common::builtins::ETH_REGISTRY_ACCOUNT_ID))
                .build(),
            DepositRequest::new_builder()
                .capacity(Pack::pack(&450_00000000u64))
                .script(receiver_script.clone())
                .registry_id(Pack::pack(&gw_common::builtins::ETH_REGISTRY_ACCOUNT_ID))
                .build(),
        ];
        let deposit_requests = deposit_requests
            .into_iter()
            .map(|d| into_deposit_info_cell(chain.generator().rollup_context(), d))
            .collect::<Vec<_>>()
            .pack();
        let produce_block_result = {
            let mem_pool = chain.mem_pool().as_ref().unwrap();
            let mut mem_pool = mem_pool.lock().await;
            construct_block(&chain, &mut mem_pool, deposit_requests.clone())
                .await
                .unwrap()
        };
        let asset_scripts = HashSet::new();
        apply_block_result(
            &mut chain,
            produce_block_result,
            deposit_requests,
            asset_scripts,
        )
        .await
        .unwrap();
        let mut db = chain.store().begin_transaction();
        let tree = BlockStateDB::from_store(&mut db, RWConfig::readonly()).unwrap();
        let sender_id = tree
            .get_account_id_by_script_hash(&sender_script.hash())
            .unwrap()
            .unwrap();
        let receiver_id = tree
            .get_account_id_by_script_hash(&receiver_script.hash())
            .unwrap()
            .unwrap();
        let receiver_script_hash = tree.get_script_hash(receiver_id).expect("get script hash");
        let receiver_address = RegistryAddress::new(
            gw_common::builtins::ETH_REGISTRY_ACCOUNT_ID,
            receiver_script_hash.as_slice()[..20].to_vec(),
        );
        let produce_block_result = {
            let args = SUDTArgs::new_builder()
                .set(SUDTArgsUnion::SUDTTransfer(
                    SUDTTransfer::new_builder()
                        .amount(Pack::pack(&U256::from(150_00000000u128)))
                        .to_address(Pack::pack(&Bytes::from(receiver_address.to_bytes())))
                        .fee(
                            Fee::new_builder()
                                .registry_id(Pack::pack(
                                    &gw_common::builtins::ETH_REGISTRY_ACCOUNT_ID,
                                ))
                                .build(),
                        )
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
            let mem_pool = chain.mem_pool().as_ref().unwrap();
            let mut mem_pool = mem_pool.lock().await;
            mem_pool.push_transaction(tx).unwrap();
            construct_block(&chain, &mut mem_pool, Default::default())
                .await
                .unwrap()
        };
        let asset_scripts = HashSet::new();
        apply_block_result(
            &mut chain,
            produce_block_result,
            Default::default(),
            asset_scripts,
        )
        .await
        .unwrap();
    }
    // deploy scripts
    let param = CellContextParam {
        stake_lock_type,
        ..Default::default()
    };
    let mut ctx = CellContext::new(&chain.generator().rollup_context().rollup_config, param);
    let challenged_block = chain.local_state().tip().clone();
    let challenge_capacity = 10000_00000000u64;
    let challenge_cell = {
        let lock_args = ChallengeLockArgs::new_builder()
            .target(
                ChallengeTarget::new_builder()
                    .target_index(Pack::pack(&0u32))
                    .target_type(ChallengeTargetType::TxExecution.into())
                    .block_hash(Pack::pack(&challenged_block.hash()))
                    .build(),
            )
            .build();
        build_rollup_locked_cell(
            &rollup_type_script.hash(),
            &challenge_script_type_hash,
            challenge_capacity,
            lock_args.as_bytes(),
        )
    };
    let global_state = chain.local_state().last_global_state();
    let initial_rollup_cell_data = global_state.as_bytes();
    // verify enter challenge
    let witness = {
        let block_proof: Bytes = {
            let mut db = chain.store().begin_transaction();
            let proof = db
                .block_smt()
                .unwrap()
                .merkle_proof(vec![challenged_block.smt_key().into()])
                .unwrap();
            proof
                .compile(vec![(challenged_block.smt_key().into())])
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
    let tx = build_simple_tx_with_out_point(
        &mut ctx.inner,
        (rollup_cell.clone(), initial_rollup_cell_data),
        input_out_point,
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

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_enter_challenge_finalized_block() {
    init_env_log();
    let input_out_point = random_out_point();
    let type_id = calculate_type_id(input_out_point.clone());
    let rollup_type_script = {
        Script::new_builder()
            .code_hash(Pack::pack(&*STATE_VALIDATOR_CODE_HASH))
            .hash_type(ScriptHashType::Data.into())
            .args(Pack::pack(&Bytes::from(type_id.to_vec())))
            .build()
    };
    // rollup lock & config
    let stake_lock_type = named_always_success_script(b"stake_lock_type_id");
    let challenge_lock_type = named_always_success_script(b"challenge_lock_type_id");
    let challenge_script_type_hash: [u8; 32] =
        challenge_lock_type.calc_script_hash().unpack().into();
    let finality_blocks = 1;
    let rollup_config = RollupConfig::new_builder()
        .challenge_script_type_hash(Pack::pack(&challenge_script_type_hash))
        .finality_blocks(Pack::pack(&finality_blocks))
        .allowed_eoa_type_hashes(
            vec![AllowedTypeHash::new(
                AllowedEoaType::Eth,
                *ALWAYS_SUCCESS_CODE_HASH,
            )]
            .pack(),
        )
        .allowed_contract_type_hashes(
            vec![AllowedTypeHash::new(AllowedContractType::Sudt, [0u8; 32])].pack(),
        )
        .build();
    // setup chain
    let mut chain = setup_chain_with_config(rollup_type_script.clone(), rollup_config).await;
    // create a rollup cell
    let capacity = 1000_00000000u64;
    let rollup_cell = build_always_success_cell(
        capacity,
        Some(ckb_types::packed::Script::new_unchecked(
            rollup_type_script.as_bytes(),
        )),
    );

    let eth_registry_id = gw_common::builtins::ETH_REGISTRY_ACCOUNT_ID;

    // deposit two account
    let rollup_script_hash = rollup_type_script.hash();
    let (sender_id, receiver_address) = {
        let mut sender_args = rollup_script_hash.to_vec();
        sender_args.extend_from_slice(&[1u8; 20]);
        let sender_script = Script::new_builder()
            .code_hash(Pack::pack(&ALWAYS_SUCCESS_CODE_HASH.clone()))
            .hash_type(ScriptHashType::Type.into())
            .args(Pack::pack(&Bytes::from(sender_args)))
            .build();
        let mut receiver_args = rollup_script_hash.to_vec();
        receiver_args.extend_from_slice(&[2u8; 20]);
        let receiver_address = RegistryAddress::new(2, [2u8; 20].to_vec());
        let receiver_script = Script::new_builder()
            .code_hash(Pack::pack(&ALWAYS_SUCCESS_CODE_HASH.clone()))
            .hash_type(ScriptHashType::Type.into())
            .args(Pack::pack(&Bytes::from(receiver_args)))
            .build();
        let rollup_ctx = chain.generator().rollup_context();
        let deposit_requests = vec![
            into_deposit_info_cell(
                rollup_ctx,
                DepositRequest::new_builder()
                    .capacity(Pack::pack(&300_00000000u64))
                    .script(sender_script.clone())
                    .registry_id(Pack::pack(&eth_registry_id))
                    .build(),
            ),
            into_deposit_info_cell(
                rollup_ctx,
                DepositRequest::new_builder()
                    .capacity(Pack::pack(&450_00000000u64))
                    .script(receiver_script.clone())
                    .registry_id(Pack::pack(&eth_registry_id))
                    .build(),
            ),
        ]
        .pack();
        let produce_block_result = {
            let mem_pool = chain.mem_pool().as_ref().unwrap();
            let mut mem_pool = mem_pool.lock().await;
            construct_block(&chain, &mut mem_pool, deposit_requests.clone())
                .await
                .unwrap()
        };
        let asset_scripts = HashSet::new();
        apply_block_result(
            &mut chain,
            produce_block_result,
            deposit_requests,
            asset_scripts,
        )
        .await
        .unwrap();
        let mut db = chain.store().begin_transaction();
        let tree = BlockStateDB::from_store(&mut db, RWConfig::readonly()).unwrap();
        let sender_id = tree
            .get_account_id_by_script_hash(&sender_script.hash())
            .unwrap()
            .unwrap();
        (sender_id, receiver_address)
    };

    // produce two blocks and challenge first one
    let mut nonce = 0u32;
    produce_block(
        &mut chain,
        &rollup_cell,
        sender_id,
        &receiver_address,
        nonce,
    )
    .await;
    nonce += 1;

    let challenged_block = chain.local_state().tip().clone();
    produce_block(
        &mut chain,
        &rollup_cell,
        sender_id,
        &receiver_address,
        nonce,
    )
    .await;

    // deploy scripts
    let param = CellContextParam {
        stake_lock_type,
        ..Default::default()
    };
    let mut ctx = CellContext::new(&chain.generator().rollup_context().rollup_config, param);
    let challenge_capacity = 10000_00000000u64;
    let challenge_cell = {
        let lock_args = ChallengeLockArgs::new_builder()
            .target(
                ChallengeTarget::new_builder()
                    .target_index(Pack::pack(&0u32))
                    .target_type(ChallengeTargetType::TxExecution.into())
                    .block_hash(Pack::pack(&challenged_block.hash()))
                    .build(),
            )
            .build();
        build_rollup_locked_cell(
            &rollup_type_script.hash(),
            &challenge_script_type_hash,
            challenge_capacity,
            lock_args.as_bytes(),
        )
    };
    let global_state = chain.local_state().last_global_state();
    let initial_rollup_cell_data = global_state.as_bytes();

    // verify enter challenge
    let witness = {
        let block_proof: Bytes = {
            let mut db = chain.store().begin_transaction();
            let proof = db
                .block_smt()
                .unwrap()
                .merkle_proof(vec![challenged_block.smt_key().into()])
                .unwrap();
            proof
                .compile(vec![(challenged_block.smt_key().into())])
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
    let tx = build_simple_tx_with_out_point(
        &mut ctx.inner,
        (rollup_cell.clone(), initial_rollup_cell_data),
        input_out_point,
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

    let err = ctx.verify_tx(tx).unwrap_err();
    let expected_err = ScriptError::ValidationFailure(
        format!(
            "by-data-hash/{}",
            ckb_types::H256(*STATE_VALIDATOR_CODE_HASH)
        ),
        INVALID_CHALLENGE_TARGET_ERROR,
    )
    .input_type_script(0);
    assert_error_eq!(err, expected_err);
}

async fn produce_block(
    chain: &mut Chain,
    _rollup_cell: &CellOutput,
    sender_id: u32,
    receiver_address: &RegistryAddress,
    nonce: u32,
) {
    let produce_block_result = {
        let args = SUDTArgs::new_builder()
            .set(SUDTArgsUnion::SUDTTransfer(
                SUDTTransfer::new_builder()
                    .amount(Pack::pack(&U256::from(50_00000000u128)))
                    .to_address(Pack::pack(&Bytes::from(receiver_address.to_bytes())))
                    .fee(
                        Fee::new_builder()
                            .registry_id(Pack::pack(&gw_common::builtins::ETH_REGISTRY_ACCOUNT_ID))
                            .build(),
                    )
                    .build(),
            ))
            .build()
            .as_bytes();
        let tx = L2Transaction::new_builder()
            .raw(
                RawL2Transaction::new_builder()
                    .from_id(Pack::pack(&sender_id))
                    .to_id(Pack::pack(&CKB_SUDT_ACCOUNT_ID))
                    .nonce(Pack::pack(&nonce))
                    .args(Pack::pack(&args))
                    .build(),
            )
            .build();
        let mem_pool = chain.mem_pool().as_ref().unwrap();
        let mut mem_pool = mem_pool.lock().await;
        mem_pool.push_transaction(tx).unwrap();
        construct_block(chain, &mut mem_pool, Default::default())
            .await
            .unwrap()
    };
    let asset_scripts = HashSet::new();
    apply_block_result(
        chain,
        produce_block_result,
        Default::default(),
        asset_scripts,
    )
    .await
    .unwrap();
}
