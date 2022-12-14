#![allow(clippy::mutable_key_type)]

use std::collections::HashSet;

use crate::script_tests::programs::STATE_VALIDATOR_CODE_HASH;
use crate::script_tests::utils::init_env_log;
use crate::script_tests::utils::layer1::build_simple_tx_with_out_point;
use crate::script_tests::utils::layer1::{always_success_script, random_out_point};
use crate::script_tests::utils::rollup::{
    build_always_success_cell, build_rollup_locked_cell, calculate_type_id,
    named_always_success_script, CellContext, CellContextParam,
};
use crate::testing_tool::chain::{
    apply_block_result, construct_block, into_deposit_info_cell, setup_chain_with_config,
    ALWAYS_SUCCESS_CODE_HASH,
};
use ckb_types::{
    packed::{CellInput, CellOutput},
    prelude::{Pack as CKBPack, Unpack as CKBUnpack},
};
use gw_common::registry_address::RegistryAddress;
use gw_common::{builtins::CKB_SUDT_ACCOUNT_ID, state::State, H256};
use gw_smt::smt::SMTH256;
use gw_smt::smt_h256_ext::SMTH256Ext;
use gw_smt::sparse_merkle_tree::default_store::DefaultStore;
use gw_store::state::history::history_state::RWConfig;
use gw_store::state::BlockStateDB;
use gw_store::traits::chain_store::ChainStore;
use gw_types::core::{AllowedContractType, AllowedEoaType, Timepoint};
use gw_types::packed::{AllowedTypeHash, Fee};
use gw_types::U256;
use gw_types::{
    bytes::Bytes,
    core::{ChallengeTargetType, ScriptHashType, Status},
    packed::{
        ChallengeLockArgs, ChallengeTarget, DepositRequest, L2Transaction, RawL2Transaction,
        RollupAction, RollupActionUnion, RollupConfig, RollupRevert, SUDTArgs, SUDTArgsUnion,
        SUDTTransfer, Script,
    },
};
use gw_types::{packed::StakeLockArgs, prelude::*};

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_revert() {
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
    let reward_receive_lock = always_success_script()
        .as_builder()
        .args(CKBPack::pack(&Bytes::from(b"reward_receive_lock".to_vec())))
        .build();
    let reward_burn_lock = ckb_types::packed::Script::new_builder()
        .args(CKBPack::pack(&Bytes::from(b"reward_burned_lock".to_vec())))
        .code_hash(CKBPack::pack(&[0u8; 32]))
        .build();
    let reward_burn_lock_hash: [u8; 32] = reward_burn_lock.calc_script_hash().unpack().into();
    let stake_lock_type = named_always_success_script(b"stake_lock_type_id");
    let stake_script_type_hash: [u8; 32] = stake_lock_type.calc_script_hash().unpack().into();
    let challenge_lock_type = named_always_success_script(b"challenge_lock_type_id");
    let challenge_script_type_hash: [u8; 32] =
        challenge_lock_type.calc_script_hash().unpack().into();
    let finality_blocks = 10;

    let rollup_config = RollupConfig::new_builder()
        .stake_script_type_hash(Pack::pack(&stake_script_type_hash))
        .challenge_script_type_hash(Pack::pack(&challenge_script_type_hash))
        .reward_burn_rate(50u8.into())
        .burn_lock_hash(Pack::pack(&reward_burn_lock_hash))
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
    let rollup_script_hash = rollup_type_script.hash();
    let eth_registry_id = gw_common::builtins::ETH_REGISTRY_ACCOUNT_ID;
    // produce a block so we can challenge it
    let prev_block_merkle = {
        // deposit two account
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
            into_deposit_info_cell(
                chain.generator().rollup_context(),
                DepositRequest::new_builder()
                    .capacity(Pack::pack(&300_00000000u64))
                    .script(sender_script.clone())
                    .registry_id(Pack::pack(&eth_registry_id))
                    .build(),
            ),
            into_deposit_info_cell(
                chain.generator().rollup_context(),
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
        let db = chain.store().begin_transaction();
        let tree = BlockStateDB::from_store(&db, RWConfig::readonly()).unwrap();
        let sender_id = tree
            .get_account_id_by_script_hash(&sender_script.hash().into())
            .unwrap()
            .unwrap();
        let receiver_address = RegistryAddress::new(1, receiver_script.hash()[0..20].to_vec());
        let produce_block_result = {
            let args = SUDTArgs::new_builder()
                .set(SUDTArgsUnion::SUDTTransfer(
                    SUDTTransfer::new_builder()
                        .amount(Pack::pack(&U256::from(150_00000000u128)))
                        .fee(
                            Fee::new_builder()
                                .amount(Pack::pack(&1_00000000u128))
                                .registry_id(Pack::pack(&eth_registry_id))
                                .build(),
                        )
                        .to_address(Pack::pack(&Bytes::from(receiver_address.to_bytes())))
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
        let prev_block_merkle = chain.local_state().last_global_state().block();
        let asset_scripts = HashSet::new();
        apply_block_result(
            &mut chain,
            produce_block_result,
            Default::default(),
            asset_scripts,
        )
        .await
        .unwrap();
        prev_block_merkle
    };
    // deploy scripts
    let param = CellContextParam {
        stake_lock_type,
        challenge_lock_type,
        ..Default::default()
    };
    let mut ctx = CellContext::new(&chain.generator().rollup_context().rollup_config, param);
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
    let challenged_block = chain.local_state().tip().clone();
    let input_challenge_cell = {
        let lock_args = ChallengeLockArgs::new_builder()
            .target(
                ChallengeTarget::new_builder()
                    .target_index(Pack::pack(&0u32))
                    .target_type(ChallengeTargetType::TxExecution.into())
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
            since |= chain
                .generator()
                .rollup_context()
                .rollup_config
                .challenge_maturity_blocks()
                .unpack();
            since
        };
        CellInput::new_builder()
            .since(CKBPack::pack(&since))
            .previous_output(out_point)
            .build()
    };
    let burn_rate: u8 = chain
        .generator()
        .rollup_context()
        .rollup_config
        .reward_burn_rate()
        .into();
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
        .local_state()
        .last_global_state()
        .clone()
        .as_builder()
        .status(Status::Halting.into())
        .build();
    let initial_rollup_cell_data = global_state.as_bytes();
    let new_tip_block = {
        let db = &chain.store().begin_transaction();
        let maybe_block = db.get_block(&challenged_block.raw().parent_block_hash().unpack());
        maybe_block.unwrap().unwrap().raw()
    };
    let new_tip_block_timestamp = new_tip_block.timestamp();
    let mut reverted_block_tree: gw_smt::smt::SMT<DefaultStore<SMTH256>> = Default::default();
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
                .compile(vec![(challenged_block.smt_key().into())])
                .unwrap()
                .0
                .into()
        };
        let reverted_block_proof: Bytes = {
            reverted_block_tree
                .merkle_proof(vec![challenged_block.hash().into()])
                .unwrap()
                .compile(vec![challenged_block.hash().into()])
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
                    .new_tip_block(new_tip_block)
                    .build(),
            ))
            .build();
        ckb_types::packed::WitnessArgs::new_builder()
            .output_type(CKBPack::pack(&Some(rollup_action.as_bytes())))
            .build()
    };
    let post_reverted_block_root = {
        reverted_block_tree
            .update(challenged_block.hash().into(), SMTH256::one())
            .unwrap();
        reverted_block_tree.root().to_h256()
    };
    let last_finalized_timepoint = {
        let number: u64 = challenged_block.raw().number().unpack();
        let finalize_blocks = chain
            .generator()
            .rollup_context()
            .rollup_config
            .finality_blocks()
            .unpack();
        Timepoint::from_block_number((number - 1).saturating_sub(finalize_blocks))
    };
    let rollup_cell_data = global_state
        .as_builder()
        .status(Status::Running.into())
        .reverted_block_root(Pack::pack(&post_reverted_block_root))
        .last_finalized_timepoint(Pack::pack(&last_finalized_timepoint.full_value()))
        .account(challenged_block.raw().prev_account())
        .block(prev_block_merkle)
        .tip_block_hash(challenged_block.raw().parent_block_hash())
        .tip_block_timestamp(new_tip_block_timestamp)
        .build()
        .as_bytes();
    let tx = build_simple_tx_with_out_point(
        &mut ctx.inner,
        (rollup_cell.clone(), initial_rollup_cell_data),
        input_out_point,
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
