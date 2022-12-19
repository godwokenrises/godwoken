#![allow(clippy::mutable_key_type)]

use std::collections::HashSet;
use std::iter::FromIterator;
use std::sync::Arc;
use std::time::SystemTime;

use crate::testing_tool::bad_block::generate_bad_block_using_first_withdrawal;
use crate::testing_tool::chain::{
    build_sync_tx, construct_block, into_deposit_info_cell, produce_empty_block,
    setup_chain_with_account_lock_manage, ALWAYS_SUCCESS_CODE_HASH, DEFAULT_FINALITY_BLOCKS,
    ETH_ACCOUNT_LOCK_CODE_HASH,
};

use ckb_types::prelude::{Builder, Entity};
use godwoken_bin::subcommand::{export_block::ExportBlock, import_block::ImportBlock};
use gw_block_producer::produce_block::ProduceBlockResult;
use gw_chain::chain::{Chain, ChallengeCell, L1Action, L1ActionContext, SyncEvent, SyncParam};
use gw_config::StoreConfig;
use gw_generator::account_lock_manage::always_success::AlwaysSuccess;
use gw_generator::account_lock_manage::secp256k1::Secp256k1Eth;
use gw_generator::account_lock_manage::AccountLockManage;
use gw_store::{readonly::StoreReadonly, schema::COLUMNS, traits::chain_store::ChainStore, Store};
use gw_types::core::{Status, Timepoint};
use gw_types::h256::*;
use gw_types::packed::DepositInfoVec;
use gw_types::{
    bytes::Bytes,
    core::{AllowedEoaType, ScriptHashType},
    offchain::CellInfo,
    packed::{
        AllowedTypeHash, CellInput, CellOutput, DepositRequest, GlobalState, OutPoint,
        RawWithdrawalRequest, RollupConfig, Script, WithdrawalRequest, WithdrawalRequestExtra,
    },
    prelude::{Pack, PackVec, Unpack},
};
use gw_utils::export_block::check_block_post_state;

const CKB: u64 = 100000000;
const MAX_MEM_BLOCK_WITHDRAWALS: u8 = 50;

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_export_import_block() {
    let _ = env_logger::builder().is_test(true).try_init();

    let always_type = random_always_success_script(None);
    let sudt_script = Script::new_builder()
        .code_hash(always_type.hash().pack())
        .hash_type(ScriptHashType::Type.into())
        .args(vec![rand::random::<u8>(), 32].pack())
        .build();

    let withdrawal_lock_type = random_always_success_script(None);
    let deposit_lock_type = random_always_success_script(None);

    let rollup_config = RollupConfig::new_builder()
        .withdrawal_script_type_hash(withdrawal_lock_type.hash().pack())
        .deposit_script_type_hash(deposit_lock_type.hash().pack())
        .l1_sudt_script_type_hash(always_type.hash().pack())
        .allowed_eoa_type_hashes(
            vec![AllowedTypeHash::new(
                AllowedEoaType::Eth,
                *ALWAYS_SUCCESS_CODE_HASH,
            )]
            .pack(),
        )
        .finality_blocks(0u64.pack())
        .build();

    let last_finalized_timepoint = Timepoint::from_block_number(100);
    let global_state = GlobalState::new_builder()
        .last_finalized_timepoint(last_finalized_timepoint.full_value().pack())
        .rollup_config_hash(rollup_config.hash().pack())
        .build();

    let state_validator_type = random_always_success_script(None);
    let rollup_type_script = Script::new_builder()
        .code_hash(state_validator_type.hash().pack())
        .hash_type(ScriptHashType::Type.into())
        .args(vec![1u8; 32].pack())
        .build();

    let rollup_script_hash: H256 = rollup_type_script.hash();
    let rollup_cell = CellInfo {
        data: global_state.as_bytes(),
        out_point: OutPoint::new_builder()
            .tx_hash(rand::random::<[u8; 32]>().pack())
            .build(),
        output: CellOutput::new_builder()
            .type_(Some(rollup_type_script.clone()).pack())
            .build(),
    };

    let store_dir = tempfile::tempdir().expect("create temp dir");
    let store = {
        let config = StoreConfig {
            path: store_dir.path().to_path_buf(),
            ..Default::default()
        };
        Store::open(&config, COLUMNS).unwrap()
    };
    let mut chain = {
        let mut account_lock_manage = AccountLockManage::default();
        account_lock_manage
            .register_lock_algorithm(*ALWAYS_SUCCESS_CODE_HASH, Arc::new(AlwaysSuccess));
        account_lock_manage.register_lock_algorithm(
            *ETH_ACCOUNT_LOCK_CODE_HASH,
            Arc::new(Secp256k1Eth::default()),
        );
        setup_chain_with_account_lock_manage(
            rollup_type_script.clone(),
            rollup_config.clone(),
            account_lock_manage,
            Some(store),
            None,
            None,
        )
        .await
    };
    let rollup_context = chain.generator().rollup_context();

    // Deposit random accounts
    const DEPOSIT_CAPACITY: u64 = 1000000 * CKB;
    const DEPOSIT_AMOUNT: u128 = 1000;
    let account_count = MAX_MEM_BLOCK_WITHDRAWALS;
    let accounts: Vec<_> = (0..account_count)
        .map(|_| {
            random_always_success_script(Some(&rollup_script_hash))
                .as_builder()
                .hash_type(ScriptHashType::Type.into())
                .build()
        })
        .collect();
    let deposits = accounts.iter().map(|account_script| {
        DepositRequest::new_builder()
            .capacity(DEPOSIT_CAPACITY.pack())
            .sudt_script_hash(sudt_script.hash().pack())
            .amount(DEPOSIT_AMOUNT.pack())
            .script(account_script.to_owned())
            .registry_id(gw_common::builtins::ETH_REGISTRY_ACCOUNT_ID.pack())
            .build()
    });
    let deposit_info_vec = DepositInfoVec::new_builder()
        .extend(deposits.map(|d| into_deposit_info_cell(rollup_context, d).pack()))
        .build();

    let deposit_block_result = {
        let mem_pool = chain.mem_pool().as_ref().unwrap();
        let mut mem_pool = mem_pool.lock().await;
        construct_block(&chain, &mut mem_pool, deposit_info_vec.clone())
            .await
            .unwrap()
    };
    let apply_deposits = L1Action {
        context: L1ActionContext::SubmitBlock {
            l2block: deposit_block_result.block.clone(),
            deposit_info_vec,
            deposit_asset_scripts: HashSet::from_iter(vec![sudt_script.clone()].into_iter()),
            withdrawals: Default::default(),
        },
        transaction: build_sync_tx(rollup_cell.output.clone(), deposit_block_result.clone()),
    };
    let param = SyncParam {
        updates: vec![apply_deposits],
        reverts: Default::default(),
    };
    chain.sync(param).await.unwrap();
    chain.notify_new_tip().await.unwrap();
    assert!(chain.last_sync_event().is_success());

    for _ in 0..DEFAULT_FINALITY_BLOCKS {
        produce_empty_block(&mut chain).await.unwrap();
    }

    // Export block
    let export_path = {
        let tmp_dir = tempfile::tempdir().expect("create temp dir");
        let mut path_buf = tmp_dir.path().to_path_buf();
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap();
        path_buf.set_file_name(format!("export_block_{}", now.as_secs()));
        path_buf
    };
    let store_readonly = StoreReadonly::open(store_dir.path(), COLUMNS).unwrap();
    let tip_block = store_readonly.get_tip_block().unwrap();
    let tip_block_number = tip_block.raw().number().unpack();
    let export_block =
        ExportBlock::new_unchecked(store_readonly, export_path.clone(), 0, tip_block_number);
    let export_store = export_block.store().clone();
    export_block.execute().unwrap();

    // Import block
    let import_store_dir = tempfile::tempdir().expect("create temp dir");
    let import_store = {
        let config = StoreConfig {
            path: import_store_dir.path().to_path_buf(),
            ..Default::default()
        };
        Store::open(&config, COLUMNS).unwrap()
    };
    let import_chain = {
        let mut account_lock_manage = AccountLockManage::default();
        account_lock_manage
            .register_lock_algorithm(*ALWAYS_SUCCESS_CODE_HASH, Arc::new(AlwaysSuccess));
        account_lock_manage.register_lock_algorithm(
            *ETH_ACCOUNT_LOCK_CODE_HASH,
            Arc::new(Secp256k1Eth::default()),
        );
        setup_chain_with_account_lock_manage(
            rollup_type_script.clone(),
            rollup_config.clone(),
            account_lock_manage,
            Some(import_store),
            None,
            None,
        )
        .await
    };
    let import_block = ImportBlock::new_unchecked(import_chain, export_path);
    let import_store = import_block.store().clone();
    import_block.execute().await.unwrap();

    // Check imported store state
    let tip_block_hash = export_store.get_tip_block_hash().unwrap();
    let tip_block = export_store.get_tip_block().unwrap();
    let tip_block_number = tip_block.raw().number().unpack();
    let post_global_state = export_store
        .get_block_post_global_state(&tip_block_hash)
        .unwrap()
        .unwrap();

    let import_tip_block_hash = import_store.get_tip_block_hash().unwrap();
    assert_eq!(tip_block_hash, import_tip_block_hash);

    let import_tx_db = import_store.begin_transaction();
    check_block_post_state(&import_tx_db, tip_block_number, &post_global_state).unwrap();

    // Test reverted block root
    generate_and_revert_a_bad_block(&mut chain, &rollup_cell, accounts[0].clone()).await;

    // Generate bad block at the same block number, test multiple block hashes at block number
    generate_and_revert_a_bad_block(&mut chain, &rollup_cell, accounts[1].clone()).await;

    // Produce new block, new global state with reverted block root updated
    produce_block(&mut chain, &rollup_cell).await;

    // Generate bad block again, reverted block iter should work correctly
    generate_and_revert_a_bad_block(&mut chain, &rollup_cell, accounts[2].clone()).await;

    // Produce new block
    produce_block(&mut chain, &rollup_cell).await;

    // Export block with reverted block root changed
    let export_path = {
        let tmp_dir = tempfile::tempdir().expect("create temp dir");
        let mut path_buf = tmp_dir.path().to_path_buf();
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap();
        path_buf.set_file_name(format!("export_block_{}", now.as_secs()));
        path_buf
    };
    // Open db again to see changes
    let store_readonly = StoreReadonly::open(store_dir.path(), COLUMNS).unwrap();
    let tip_block = store_readonly.get_tip_block().unwrap();
    let tip_block_number = tip_block.raw().number().unpack();
    let export_block =
        ExportBlock::new_unchecked(store_readonly, export_path.clone(), 0, tip_block_number);
    let export_store = export_block.store().clone();
    export_block.execute().unwrap();

    // Import block
    let import_store_dir = tempfile::tempdir().expect("create temp dir");
    let import_store = {
        let config = StoreConfig {
            path: import_store_dir.path().to_path_buf(),
            ..Default::default()
        };
        Store::open(&config, COLUMNS).unwrap()
    };
    let import_chain = {
        let mut account_lock_manage = AccountLockManage::default();
        account_lock_manage
            .register_lock_algorithm(*ALWAYS_SUCCESS_CODE_HASH, Arc::new(AlwaysSuccess));
        account_lock_manage.register_lock_algorithm(
            *ETH_ACCOUNT_LOCK_CODE_HASH,
            Arc::new(Secp256k1Eth::default()),
        );
        setup_chain_with_account_lock_manage(
            rollup_type_script.clone(),
            rollup_config.clone(),
            account_lock_manage,
            Some(import_store),
            None,
            None,
        )
        .await
    };
    let import_block = ImportBlock::new_unchecked(import_chain, export_path);
    let import_store = import_block.store().clone();
    import_block.execute().await.unwrap();

    // Check imported store state
    let tip_block_hash = export_store.get_tip_block_hash().unwrap();
    let tip_block = export_store.get_tip_block().unwrap();
    let tip_block_number = tip_block.raw().number().unpack();
    let post_global_state = export_store
        .get_block_post_global_state(&tip_block_hash)
        .unwrap()
        .unwrap();

    let reverted_block_root: H256 = post_global_state.reverted_block_root().unpack();
    assert!(!reverted_block_root.is_zero());

    let import_tip_block_hash = import_store.get_tip_block_hash().unwrap();
    assert_eq!(tip_block_hash, import_tip_block_hash);

    let import_tx_db = import_store.begin_transaction();
    check_block_post_state(&import_tx_db, tip_block_number, &post_global_state).unwrap();
}

async fn generate_and_revert_a_bad_block(
    chain: &mut Chain,
    rollup_cell: &CellInfo,
    account_script: Script,
) {
    let prev_tip_block_number = chain.local_state().tip().raw().number().unpack();

    // update bad block
    let withdrawal = {
        let owner_lock = Script::default();
        let raw = RawWithdrawalRequest::new_builder()
            .capacity((1000 * CKB).pack())
            .account_script_hash(account_script.hash().pack())
            .sudt_script_hash(H256::zero().pack())
            .owner_lock_hash(owner_lock.hash().pack())
            .registry_id(gw_common::builtins::ETH_REGISTRY_ACCOUNT_ID.pack())
            .build();
        let withdrawal = WithdrawalRequest::new_builder().raw(raw).build();
        WithdrawalRequestExtra::new_builder()
            .request(withdrawal)
            .owner_lock(owner_lock)
            .build()
    };
    let block_result = {
        let mem_pool = chain.mem_pool().as_ref().unwrap();
        let mut mem_pool = mem_pool.lock().await;
        mem_pool.push_withdrawal_request(withdrawal).await.unwrap();
        construct_block(chain, &mut mem_pool, Default::default())
            .await
            .unwrap()
    };
    let bad_block_result = {
        let ProduceBlockResult {
            block,
            global_state,
            withdrawal_extras,
            deposit_cells,
            remaining_capacity,
        } = block_result.clone();
        let (bad_block, bad_global_state) =
            generate_bad_block_using_first_withdrawal(chain, block, global_state);
        let withdrawal_extras = withdrawal_extras
            .into_iter()
            .enumerate()
            .map(|(i, withdraw)| {
                withdraw
                    .as_builder()
                    .request(bad_block.withdrawals().get(i).unwrap())
                    .build()
            })
            .collect();
        ProduceBlockResult {
            block: bad_block,
            global_state: bad_global_state,
            withdrawal_extras,
            deposit_cells,
            remaining_capacity,
        }
    };

    let update_bad_block = L1Action {
        context: L1ActionContext::SubmitBlock {
            l2block: bad_block_result.block.clone(),
            deposit_info_vec: Default::default(),
            deposit_asset_scripts: Default::default(),
            withdrawals: bad_block_result.withdrawal_extras.clone(),
        },
        transaction: build_sync_tx(rollup_cell.output.clone(), bad_block_result.clone()),
    };
    let param = SyncParam {
        updates: vec![update_bad_block],
        reverts: Default::default(),
    };
    chain.sync(param).await.unwrap();
    assert!(matches!(
        chain.last_sync_event(),
        SyncEvent::BadBlock { .. }
    ));

    let tip_block = chain.store().get_tip_block().unwrap();
    let tip_block_number: u64 = tip_block.raw().number().unpack();
    assert_eq!(tip_block_number, prev_tip_block_number + 1);

    // challenge bad block
    let challenge_context = match chain.last_sync_event() {
        SyncEvent::BadBlock { context } => context.to_owned(),
        _ => unreachable!(),
    };
    let challenge_cell = ChallengeCell {
        input: CellInput::default(),
        output: CellOutput::default(),
        output_data: Bytes::default(),
    };

    let bad_block_result = ProduceBlockResult {
        global_state: bad_block_result
            .global_state
            .as_builder()
            .status(Status::Halting.into())
            .build(),
        ..bad_block_result
    };

    let challenge_bad_block = L1Action {
        context: L1ActionContext::Challenge {
            cell: challenge_cell,
            target: challenge_context.target,
            witness: challenge_context.witness,
        },
        transaction: build_sync_tx(rollup_cell.output.clone(), bad_block_result.clone()),
    };
    let param = SyncParam {
        updates: vec![challenge_bad_block],
        reverts: Default::default(),
    };
    chain.sync(param).await.unwrap();
    assert!(matches!(
        chain.last_sync_event(),
        SyncEvent::WaitChallenge { .. }
    ));

    // Revert bad block
    let reverted_block_smt_root = match chain.last_sync_event() {
        SyncEvent::WaitChallenge { cell: _, context } => context.post_reverted_block_root,
        _ => unreachable!(),
    };
    let db = &chain.store().begin_transaction();
    let last_valid_tip_block_hash = db.get_last_valid_tip_block_hash().unwrap();
    let last_valid_tip_block = db.get_last_valid_tip_block().unwrap();
    let block_smt = {
        let global_state = db
            .get_block_post_global_state(&last_valid_tip_block_hash)
            .unwrap();
        global_state.unwrap().block()
    };
    let reverted_block_result = ProduceBlockResult {
        global_state: bad_block_result
            .global_state
            .as_builder()
            .status(Status::Running.into())
            .reverted_block_root(reverted_block_smt_root.pack())
            .tip_block_hash(last_valid_tip_block_hash.pack())
            .block(block_smt)
            .account(last_valid_tip_block.raw().post_account())
            .build(),
        ..bad_block_result
    };

    let revert_bad_block = L1Action {
        context: L1ActionContext::Revert {
            reverted_blocks: vec![reverted_block_result.block.raw()],
        },
        transaction: build_sync_tx(rollup_cell.output.clone(), reverted_block_result),
    };
    let param = SyncParam {
        updates: vec![revert_bad_block],
        reverts: Default::default(),
    };
    chain.sync(param).await.unwrap();
    assert!(chain.last_sync_event().is_success());

    let local_reverted_block_smt_root = db.get_reverted_block_smt_root().unwrap();
    assert_eq!(local_reverted_block_smt_root, reverted_block_smt_root);
}

async fn produce_block(chain: &mut Chain, rollup_cell: &CellInfo) {
    let block_result = {
        let mem_pool = chain.mem_pool().as_ref().unwrap();
        let mut mem_pool = mem_pool.lock().await;
        construct_block(chain, &mut mem_pool, Default::default())
            .await
            .unwrap()
    };
    let new_block = L1Action {
        context: L1ActionContext::SubmitBlock {
            l2block: block_result.block.clone(),
            deposit_info_vec: Default::default(),
            deposit_asset_scripts: Default::default(),
            withdrawals: block_result.withdrawal_extras.clone(),
        },
        transaction: build_sync_tx(rollup_cell.output.clone(), block_result),
    };
    let param = SyncParam {
        updates: vec![new_block],
        reverts: Default::default(),
    };
    chain.sync(param).await.unwrap();
    chain.notify_new_tip().await.unwrap();
    assert!(chain.last_sync_event().is_success());
}

fn random_always_success_script(opt_rollup_script_hash: Option<&H256>) -> Script {
    let random_bytes: [u8; 20] = rand::random();
    Script::new_builder()
        .code_hash(ALWAYS_SUCCESS_CODE_HASH.clone().pack())
        .hash_type(ScriptHashType::Data.into())
        .args({
            let mut args = opt_rollup_script_hash
                .map(|h| h.as_slice().to_vec())
                .unwrap_or_default();
            args.extend_from_slice(&random_bytes);
            args.pack()
        })
        .build()
}
