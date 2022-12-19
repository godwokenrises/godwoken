#![allow(clippy::mutable_key_type)]

use std::collections::HashSet;
use std::sync::Arc;

use crate::script_tests::programs::STATE_VALIDATOR_CODE_HASH;
use crate::script_tests::utils::init_env_log;
use crate::script_tests::utils::layer1::build_simple_tx_with_out_point;
use crate::script_tests::utils::layer1::random_out_point;
use crate::script_tests::utils::rollup::{
    build_always_success_cell, build_rollup_locked_cell, calculate_type_id,
    named_always_success_script, CellContext, CellContextParam,
};
use crate::testing_tool::chain::into_deposit_info_cell;
use crate::testing_tool::chain::setup_chain_with_account_lock_manage;
use crate::testing_tool::chain::{apply_block_result, construct_block};
use ckb_types::{
    packed::{CellInput, CellOutput},
    prelude::{Pack as CKBPack, Unpack as CKBUnpack},
};
use gw_common::builtins::ETH_REGISTRY_ACCOUNT_ID;
use gw_common::merkle_utils::ckb_merkle_leaf_hash;
use gw_common::registry_address::RegistryAddress;
use gw_common::state::State;
use gw_generator::account_lock_manage::always_success::AlwaysSuccess;
use gw_generator::account_lock_manage::eip712;
use gw_generator::account_lock_manage::eip712::traits::EIP712Encode;
use gw_generator::account_lock_manage::eip712::types::EIP712Domain;
use gw_generator::account_lock_manage::AccountLockManage;
use gw_store::smt::smt_store::SMTStateStore;
use gw_store::state::history::history_state::RWConfig;
use gw_store::state::traits::JournalDB;
use gw_store::state::BlockStateDB;
use gw_store::state::MemStateDB;
use gw_traits::CodeStore;
use gw_types::core::AllowedContractType;
use gw_types::core::AllowedEoaType;
use gw_types::core::SigningType;
use gw_types::h256::*;
use gw_types::packed::AllowedTypeHash;
use gw_types::packed::CCTransactionSignatureWitness;
use gw_types::packed::Fee;
use gw_types::prelude::*;
use gw_types::U256;
use gw_types::{
    bytes::Bytes,
    core::{ChallengeTargetType, ScriptHashType, Status},
    packed::{
        ChallengeLockArgs, ChallengeTarget, DepositRequest, L2Transaction, RawL2Transaction,
        RollupAction, RollupActionUnion, RollupCancelChallenge, RollupConfig, SUDTArgs,
        SUDTTransfer, Script,
    },
};

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_cancel_tx_signature() {
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
    let eoa_lock_type = named_always_success_script(b"eoa_lock_type_id");
    let l2_sudt_type = named_always_success_script(b"l2_sudt_type_id");
    let challenge_script_type_hash: [u8; 32] =
        challenge_lock_type.calc_script_hash().unpack().into();
    let eoa_lock_type_hash: [u8; 32] = eoa_lock_type.calc_script_hash().unpack().into();
    let l2_sudt_type_hash: [u8; 32] = l2_sudt_type.calc_script_hash().unpack().into();

    let allowed_eoa_type_hashes: Vec<AllowedTypeHash> = vec![AllowedTypeHash::new(
        AllowedEoaType::Eth,
        eoa_lock_type_hash,
    )];
    let finality_blocks = 10;
    let rollup_config = RollupConfig::new_builder()
        .challenge_script_type_hash(Pack::pack(&challenge_script_type_hash))
        .allowed_eoa_type_hashes(PackVec::pack(allowed_eoa_type_hashes))
        .l2_sudt_validator_script_type_hash(Pack::pack(&l2_sudt_type_hash))
        .allowed_contract_type_hashes(PackVec::pack(vec![AllowedTypeHash::from_unknown(
            l2_sudt_type_hash,
        )]))
        .allowed_contract_type_hashes(
            vec![AllowedTypeHash::new(
                AllowedContractType::Sudt,
                l2_sudt_type_hash,
            )]
            .pack(),
        )
        .finality_blocks(Pack::pack(&finality_blocks))
        .build();
    // setup chain
    let mut account_lock_manage = AccountLockManage::default();
    account_lock_manage.register_lock_algorithm(eoa_lock_type_hash, Arc::new(AlwaysSuccess));
    let mut chain = setup_chain_with_account_lock_manage(
        rollup_type_script.clone(),
        rollup_config.clone(),
        account_lock_manage,
        None,
        None,
        None,
    )
    .await;
    chain
        .mem_pool()
        .as_ref()
        .unwrap()
        .lock()
        .await
        .mem_pool_state()
        .set_completed_initial_syncing();
    // create a rollup cell
    let capacity = 1000_00000000u64;
    let rollup_cell = build_always_success_cell(
        capacity,
        Some(ckb_types::packed::Script::new_unchecked(
            rollup_type_script.as_bytes(),
        )),
    );
    // CKB built-in account id
    let sudt_id = 1;
    let rollup_script_hash = rollup_type_script.hash();
    let eth_registry_id = gw_common::builtins::ETH_REGISTRY_ACCOUNT_ID;
    // produce a block so we can challenge it
    let (sender_script, receiver_script, sudt_script) = {
        // deposit two account
        let mut sender_args = rollup_script_hash.to_vec();
        sender_args.extend_from_slice(&[1u8; 20]);
        let sender_script = Script::new_builder()
            .code_hash(Pack::pack(&eoa_lock_type_hash.clone()))
            .hash_type(ScriptHashType::Type.into())
            .args(Pack::pack(&Bytes::from(sender_args)))
            .build();
        let mut receiver_args = rollup_script_hash.to_vec();
        receiver_args.extend_from_slice(&[2u8; 20]);
        let receiver_script = Script::new_builder()
            .code_hash(Pack::pack(&eoa_lock_type_hash.clone()))
            .hash_type(ScriptHashType::Type.into())
            .args(Pack::pack(&Bytes::from(receiver_args)))
            .build();
        let receiver_address = RegistryAddress::new(eth_registry_id, vec![2u8; 20]);
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
        let db = chain.store().begin_transaction();
        let tree = BlockStateDB::from_store(&db, RWConfig::readonly()).unwrap();
        let sender_id = tree
            .get_account_id_by_script_hash(&sender_script.hash())
            .unwrap()
            .unwrap();
        let sudt_script_hash = tree.get_script_hash(sudt_id).unwrap();
        let sudt_script = tree.get_script(&sudt_script_hash).unwrap();
        let transfer_capacity = 2_00000000u128;
        let fee_capacity = 1_00000000u128;
        let args = SUDTArgs::new_builder()
            .set(
                SUDTTransfer::new_builder()
                    .to_address(Pack::pack(&Bytes::from(receiver_address.to_bytes())))
                    .amount(Pack::pack(&U256::from(transfer_capacity)))
                    .fee(
                        Fee::new_builder()
                            .amount(Pack::pack(&fee_capacity))
                            .registry_id(Pack::pack(&receiver_address.registry_id))
                            .build(),
                    )
                    .build(),
            )
            .build()
            .as_bytes();
        let tx = L2Transaction::new_builder()
            .raw(
                RawL2Transaction::new_builder()
                    .from_id(Pack::pack(&sender_id))
                    .to_id(Pack::pack(&sudt_id))
                    .nonce(Pack::pack(&0u32))
                    .args(Pack::pack(&args))
                    .build(),
            )
            .build();
        let produce_block_result = {
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
        (sender_script, receiver_script, sudt_script)
    };
    // deploy scripts
    let param = CellContextParam {
        stake_lock_type,
        challenge_lock_type,
        eoa_lock_type,
        l2_sudt_type,
        ..Default::default()
    };
    let mut ctx = CellContext::new(&rollup_config, param);
    let challenge_capacity = 10000_00000000u64;
    let challenged_block = chain.local_state().tip().clone();
    let challenge_target_index = 0u32;
    let tx = challenged_block
        .transactions()
        .get(challenge_target_index as usize)
        .unwrap();

    let input_challenge_cell = {
        let lock_args = ChallengeLockArgs::new_builder()
            .target(
                ChallengeTarget::new_builder()
                    .target_index(Pack::pack(&challenge_target_index))
                    .target_type(ChallengeTargetType::TxSignature.into())
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
        let out_point = ctx.insert_cell(cell, Bytes::default());
        CellInput::new_builder().previous_output(out_point).build()
    };
    let global_state = chain
        .local_state()
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
    let sender_address;
    let challenge_witness = {
        let witness = {
            let leaves: Vec<H256> = challenged_block
                .transactions()
                .into_iter()
                .enumerate()
                .map(|(idx, tx)| ckb_merkle_leaf_hash(idx as u32, &tx.witness_hash()))
                .collect();
            let tx_proof = super::build_merkle_proof(&leaves, &[challenge_target_index]);
            let challenged_block_number =
                gw_types::prelude::Unpack::unpack(&challenged_block.raw().number());

            // Detach block to get right state snapshot
            let db = chain.store().begin_transaction();
            {
                db.detach_block(&challenged_block).unwrap();
                {
                    let mut tree = BlockStateDB::from_store(&db, RWConfig::detach_block()).unwrap();
                    tree.detach_block_state(challenged_block_number).unwrap();
                }
            }
            db.commit().unwrap();

            let mut tree = MemStateDB::from_store(chain.store().get_snapshot()).unwrap();
            tree.set_state_tracker(Default::default());
            let sender_id = tree
                .get_account_id_by_script_hash(&sender_script.hash())
                .unwrap()
                .unwrap();
            sender_address = tree
                .get_registry_address_by_script_hash(ETH_REGISTRY_ACCOUNT_ID, &sender_script.hash())
                .unwrap()
                .expect("get sender address");
            tree.get_script_hash(sender_id).unwrap();
            tree.get_nonce(sender_id).unwrap();
            let receiver_id = tree
                .get_account_id_by_script_hash(&receiver_script.hash())
                .unwrap()
                .unwrap();
            tree.get_script_hash(receiver_id).unwrap();
            tree.get_nonce(receiver_id).unwrap();
            tree.get_script_hash(sudt_id).unwrap();
            let account_count = tree.get_account_count().unwrap();
            let touched_keys: Vec<H256> = {
                let keys = tree.state_tracker().unwrap().touched_keys();
                let unlock = keys.lock().unwrap();
                unlock.clone().into_iter().collect()
            };

            let kv_state = touched_keys
                .iter()
                .map(|k| {
                    let v = tree.get_raw(k).unwrap();
                    (*k, v)
                })
                .collect::<Vec<(H256, H256)>>();

            let kv_state_proof: Bytes = {
                let smt = SMTStateStore::new(&db).to_smt().unwrap();
                let smt_touched_keys: Vec<_> = touched_keys.iter().map(|k| (*k).into()).collect();
                smt.merkle_proof(smt_touched_keys.clone())
                    .unwrap()
                    .compile(smt_touched_keys)
                    .unwrap()
                    .0
                    .into()
            };
            CCTransactionSignatureWitness::new_builder()
                .l2tx(tx.clone())
                .raw_l2block(challenged_block.raw())
                .kv_state_proof(Pack::pack(&kv_state_proof))
                .tx_proof(tx_proof)
                .sender(sender_script.clone())
                .receiver(sudt_script.clone())
                .account_count(Pack::pack(&account_count))
                .kv_state(kv_state.pack())
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
        let owner_lock_hash = vec![42u8; 32];
        let message = {
            let typed_tx = eip712::types::L2Transaction::from_raw(
                &tx.raw(),
                sender_address,
                sudt_script.hash(),
            )
            .unwrap();
            let domain_seperator = EIP712Domain {
                name: "Godwoken".to_string(),
                version: "1".to_string(),
                chain_id: Unpack::unpack(&tx.raw().chain_id()),
                verifying_contract: None,
                salt: None,
            };
            typed_tx.eip712_message(domain_seperator.hash_struct())
        };
        let data: Bytes = {
            let mut buf = owner_lock_hash.to_vec();
            buf.push(SigningType::Raw.into());
            buf.extend_from_slice(&message);
            buf.into()
        };
        let out_point = ctx.insert_cell(cell, data);
        CellInput::new_builder().previous_output(out_point).build()
    };
    let rollup_cell_data = global_state
        .as_builder()
        .status(Status::Running.into())
        .build()
        .as_bytes();
    let tx = build_simple_tx_with_out_point(
        &mut ctx.inner,
        (rollup_cell.clone(), initial_rollup_cell_data),
        input_out_point,
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
    .cell_dep(ctx.l2_sudt_dep.clone())
    .build();
    ctx.verify_tx(tx).expect("return success");
}
