#![allow(clippy::mutable_key_type)]

use std::collections::HashSet;
use std::sync::Arc;

use crate::script_tests::utils::init_env_log;
use crate::script_tests::utils::layer1::build_simple_tx_with_out_point;
use crate::script_tests::utils::layer1::random_out_point;
use crate::script_tests::utils::rollup::{
    build_always_success_cell, build_rollup_locked_cell, build_type_id_script,
    calculate_state_validator_type_id, CellContext, CellContextParam,
};
use crate::testing_tool::chain::setup_chain_with_account_lock_manage;
use crate::testing_tool::chain::{apply_block_result, construct_block};
use crate::testing_tool::programs::STATE_VALIDATOR_CODE_HASH;
use ckb_types::{
    packed::{CellInput, CellOutput},
    prelude::{Pack as CKBPack, Unpack as CKBUnpack},
};
use gw_common::builtins::ETH_REGISTRY_ACCOUNT_ID;
use gw_common::merkle_utils::ckb_merkle_leaf_hash;
use gw_common::merkle_utils::CBMT;
use gw_common::state::State;
use gw_common::H256;
use gw_generator::account_lock_manage::always_success::AlwaysSuccess;
use gw_generator::account_lock_manage::eip712::{
    traits::EIP712Encode,
    types::{EIP712Domain, Withdrawal},
};
use gw_generator::account_lock_manage::AccountLockManage;
use gw_store::mem_pool_state::MemPoolState;
use gw_store::mem_pool_state::MemStore;
use gw_types::core::AllowedEoaType;
use gw_types::core::SigningType;
use gw_types::packed::AllowedTypeHash;
use gw_types::packed::CCWithdrawalWitness;
use gw_types::packed::WithdrawalRequestExtra;
use gw_types::prelude::Pack as GWPack;
use gw_types::prelude::*;
use gw_types::{
    bytes::Bytes,
    core::{ChallengeTargetType, ScriptHashType, Status},
    packed::{
        CKBMerkleProof, ChallengeLockArgs, ChallengeTarget, DepositRequest, RawWithdrawalRequest,
        RollupAction, RollupActionUnion, RollupCancelChallenge, RollupConfig, Script,
        WithdrawalRequest,
    },
};

mod tx_execution;
mod tx_signature;
mod withdrawal;

pub(crate) fn build_merkle_proof(leaves: &[H256], indices: &[u32]) -> CKBMerkleProof {
    let proof = CBMT::build_merkle_proof(leaves, indices).unwrap();
    CKBMerkleProof::new_builder()
        .lemmas(proof.lemmas().pack())
        .indices(GWPack::pack(proof.indices()))
        .build()
}

// Cancel withdrawal signature challengen
#[tokio::test]
async fn test_burn_challenge_capacity() {
    init_env_log();
    let input_out_point = random_out_point();
    let type_id = calculate_state_validator_type_id(input_out_point.clone());
    let rollup_type_script = {
        Script::new_builder()
            .code_hash(Pack::pack(&*STATE_VALIDATOR_CODE_HASH))
            .hash_type(ScriptHashType::Data.into())
            .args(Pack::pack(&Bytes::from(type_id.to_vec())))
            .build()
    };
    // rollup lock & config
    let reward_burn_lock = ckb_types::packed::Script::new_builder()
        .args(CKBPack::pack(&Bytes::from(b"reward_burned_lock".to_vec())))
        .code_hash(CKBPack::pack(&[0u8; 32]))
        .build();
    let reward_burn_lock_hash: [u8; 32] = reward_burn_lock.calc_script_hash().unpack();
    let stake_lock_type = build_type_id_script(b"stake_lock_type_id");
    let challenge_lock_type = build_type_id_script(b"challenge_lock_type_id");
    let eoa_lock_type = build_type_id_script(b"eoa_lock_type_id");
    let challenge_script_type_hash: [u8; 32] = challenge_lock_type.calc_script_hash().unpack();
    let eoa_lock_type_hash: [u8; 32] = eoa_lock_type.calc_script_hash().unpack();
    let allowed_eoa_type_hashes: Vec<AllowedTypeHash> = vec![AllowedTypeHash::new(
        AllowedEoaType::Eth,
        eoa_lock_type_hash,
    )];
    let finality_blocks = 10;
    let rollup_config = RollupConfig::new_builder()
        .challenge_script_type_hash(Pack::pack(&challenge_script_type_hash))
        .reward_burn_rate(50u8.into())
        .burn_lock_hash(Pack::pack(&reward_burn_lock_hash))
        .allowed_eoa_type_hashes(PackVec::pack(allowed_eoa_type_hashes))
        .finality_blocks(Pack::pack(&finality_blocks))
        .build();
    // setup chain
    let mut account_lock_manage = AccountLockManage::default();
    account_lock_manage.register_lock_algorithm(eoa_lock_type_hash.into(), Box::new(AlwaysSuccess));
    let mut chain = setup_chain_with_account_lock_manage(
        rollup_type_script.clone(),
        rollup_config.clone(),
        account_lock_manage,
    )
    .await;
    chain.complete_initial_syncing().await.unwrap();
    // create a rollup cell
    let capacity = 1000_00000000u64;
    let rollup_cell = build_always_success_cell(
        capacity,
        Some(ckb_types::packed::Script::new_unchecked(
            rollup_type_script.as_bytes(),
        )),
    );
    let eth_registry_id = gw_common::builtins::ETH_REGISTRY_ACCOUNT_ID;
    let withdrawal_extra;
    // produce a block so we can challenge it
    let sender_script = {
        // deposit two account
        let mut sender_args = rollup_type_script.hash().to_vec();
        sender_args.extend_from_slice(&[1u8; 20]);
        let sender_script = Script::new_builder()
            .code_hash(Pack::pack(&eoa_lock_type_hash.clone()))
            .hash_type(ScriptHashType::Type.into())
            .args(Pack::pack(&Bytes::from(sender_args)))
            .build();
        let mut receiver_args = rollup_type_script.hash().to_vec();
        receiver_args.extend_from_slice(&[2u8; 20]);
        let receiver_script = Script::new_builder()
            .code_hash(Pack::pack(&eoa_lock_type_hash.clone()))
            .hash_type(ScriptHashType::Type.into())
            .args(Pack::pack(&Bytes::from(receiver_args)))
            .build();
        let deposit_requests = vec![
            DepositRequest::new_builder()
                .capacity(Pack::pack(&450_00000000u64))
                .script(sender_script.clone())
                .registry_id(Pack::pack(&eth_registry_id))
                .build(),
            DepositRequest::new_builder()
                .capacity(Pack::pack(&550_00000000u64))
                .script(receiver_script)
                .registry_id(Pack::pack(&eth_registry_id))
                .build(),
        ];
        let produce_block_result = {
            let mem_pool = chain.mem_pool().as_ref().unwrap();
            let mut mem_pool = mem_pool.lock().await;
            construct_block(&chain, &mut mem_pool, deposit_requests.clone())
                .await
                .unwrap()
        };
        let rollup_cell = gw_types::packed::CellOutput::new_unchecked(rollup_cell.as_bytes());
        let asset_scripts = HashSet::new();
        apply_block_result(
            &mut chain,
            rollup_cell.clone(),
            produce_block_result,
            deposit_requests,
            asset_scripts,
        )
        .await;

        let withdrawal_capacity = 365_00000000u64;
        withdrawal_extra = {
            let owner_lock = Script::default();
            WithdrawalRequestExtra::new_builder()
                .request(
                    WithdrawalRequest::new_builder()
                        .raw(
                            RawWithdrawalRequest::new_builder()
                                .nonce(Pack::pack(&0u32))
                                .capacity(Pack::pack(&withdrawal_capacity))
                                .account_script_hash(Pack::pack(&sender_script.hash()))
                                .owner_lock_hash(Pack::pack(&owner_lock.hash()))
                                .registry_id(Pack::pack(&eth_registry_id))
                                .build(),
                        )
                        .build(),
                )
                .owner_lock(owner_lock)
                .build()
        };
        let produce_block_result = {
            let mem_pool = chain.mem_pool().as_ref().unwrap();
            let mut mem_pool = mem_pool.lock().await;
            mem_pool
                .push_withdrawal_request(withdrawal_extra.clone())
                .await
                .unwrap();
            construct_block(&chain, &mut mem_pool, Vec::default())
                .await
                .unwrap()
        };

        let asset_scripts = HashSet::new();
        apply_block_result(
            &mut chain,
            rollup_cell,
            produce_block_result,
            vec![],
            asset_scripts,
        )
        .await;
        sender_script
    };
    // deploy scripts
    let param = CellContextParam {
        stake_lock_type,
        challenge_lock_type,
        eoa_lock_type,
        ..Default::default()
    };
    let mut ctx = CellContext::new(&rollup_config, param);
    let challenge_capacity = 10000_00000000u64;
    let challenged_block = chain.local_state().tip().clone();
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
    let burn_rate: u8 = rollup_config.reward_burn_rate().into();
    let burned_capacity: u64 = challenge_capacity * burn_rate as u64 / 100;
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

    let state = {
        let mem_store = MemStore::new(chain.store().get_snapshot());
        MemPoolState::new(Arc::new(mem_store), true)
    };
    let snap = state.load();
    let mut tree = snap.state().unwrap();

    tree.tracker_mut().enable();
    let withdrawal_address = tree
        .get_registry_address_by_script_hash(ETH_REGISTRY_ACCOUNT_ID, &sender_script.hash().into())
        .unwrap()
        .unwrap();
    let sender_id = tree
        .get_account_id_by_script_hash(&sender_script.hash().into())
        .unwrap()
        .unwrap();
    tree.get_script_hash(sender_id).unwrap();
    tree.get_nonce(sender_id).unwrap();
    let account_count = tree.get_account_count().unwrap();
    let touched_keys: Vec<H256> = {
        let keys = tree.tracker_mut().touched_keys().unwrap();
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
        let db = chain.store().begin_transaction();
        let account_smt = db.account_smt().unwrap();
        account_smt
            .merkle_proof(touched_keys)
            .unwrap()
            .compile(kv_state.clone())
            .unwrap()
            .0
            .into()
    };
    let challenge_witness = {
        let witness = {
            // build proof
            let leaves: Vec<H256> = challenged_block
                .withdrawals()
                .into_iter()
                .enumerate()
                .map(|(idx, withdrawal)| {
                    let hash: H256 = withdrawal.witness_hash().into();
                    ckb_merkle_leaf_hash(idx as u32, &hash)
                })
                .collect();

            let proof = build_merkle_proof(&leaves, &[challenge_target_index]);
            // we do not actually execute the signature verification in this test
            CCWithdrawalWitness::new_builder()
                .raw_l2block(challenged_block.raw())
                .withdrawal(withdrawal.clone())
                .withdrawal_proof(proof)
                .owner_lock(withdrawal_extra.owner_lock())
                .sender(sender_script.clone())
                .kv_state_proof(Pack::pack(&kv_state_proof))
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
            let withdrawal = Withdrawal::from_raw(
                withdrawal.raw(),
                withdrawal_extra.owner_lock(),
                withdrawal_address,
            )
            .unwrap();
            let domain = EIP712Domain {
                name: "Godwoken".to_string(),
                version: "1".to_string(),
                chain_id: withdrawal_extra.raw().chain_id().unpack(),
                verifying_contract: None,
                salt: None,
            };
            withdrawal.eip712_message(domain.hash_struct())
        };
        let mut buf = owner_lock_hash;
        buf.push(SigningType::Raw.into());
        buf.extend_from_slice(&message);
        let out_point = ctx.insert_cell(cell, Bytes::from(buf));
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
    .output(reward_burned_cell)
    .output_data(Default::default())
    .cell_dep(ctx.challenge_lock_dep.clone())
    .cell_dep(ctx.stake_lock_dep.clone())
    .cell_dep(ctx.always_success_dep.clone())
    .cell_dep(ctx.state_validator_dep.clone())
    .cell_dep(ctx.rollup_config_dep.clone())
    .cell_dep(ctx.eoa_lock_dep.clone())
    .build();
    ctx.verify_tx(tx).expect("return success");
}
