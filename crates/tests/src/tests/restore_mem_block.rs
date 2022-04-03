use std::sync::Arc;
use std::time::Duration;

use crate::testing_tool::chain::{
    build_sync_tx, chain_generator, construct_block, restart_chain, setup_chain,
    ALWAYS_SUCCESS_CODE_HASH,
};
use crate::testing_tool::mem_pool_provider::DummyMemPoolProvider;

use ckb_types::prelude::{Builder, Entity};
use gw_block_producer::test_mode_control::TestModeControl;
use gw_chain::chain::{L1Action, L1ActionContext, SyncParam};
use gw_common::{
    state::{to_short_address, State},
    H256,
};
use gw_config::RPCClientConfig;
use gw_dynamic_config::manager::DynamicConfigManager;
use gw_generator::ArcSwap;
use gw_rpc_client::ckb_client::CKBClient;
use gw_rpc_client::indexer_client::CKBIndexerClient;
use gw_rpc_client::rpc_client::RPCClient;
use gw_rpc_server::registry::{Registry, RegistryArgs};
use gw_store::state::state_db::StateContext;
use gw_types::core::ScriptHashType;
use gw_types::offchain::{CellInfo, CollectedCustodianCells, DepositInfo, RollupContext};
use gw_types::packed::{
    CellOutput, DepositLockArgs, DepositRequest, L2BlockCommittedInfo, L2Transaction, OutPoint,
    RawL2Transaction, RawWithdrawalRequest, SUDTArgs, SUDTTransfer, Script, WithdrawalRequest,
};
use gw_types::prelude::Pack;

const CKB: u64 = 100000000;

#[tokio::test]
async fn test_restore_mem_block() {
    let _ = env_logger::builder().is_test(true).try_init();

    let rollup_type_script = Script::default();
    let rollup_script_hash: H256 = rollup_type_script.hash().into();
    let rollup_cell = CellOutput::new_builder()
        .type_(Some(rollup_type_script.clone()).pack())
        .build();
    let mut chain = setup_chain(rollup_type_script.clone()).await;

    // Deposit 20 accounts
    const DEPOSIT_CAPACITY: u64 = 1000000 * CKB;
    let accounts: Vec<_> = (0..20)
        .map(|_| random_always_success_script(&rollup_script_hash))
        .collect();
    let deposits = accounts.iter().map(|account_script| {
        DepositRequest::new_builder()
            .capacity(DEPOSIT_CAPACITY.pack())
            .sudt_script_hash(H256::zero().pack())
            .amount(0.pack())
            .script(account_script.to_owned())
            .build()
    });

    let block_result = {
        let mem_pool = chain.mem_pool().as_ref().unwrap();
        let mut mem_pool = mem_pool.lock().await;
        construct_block(&chain, &mut mem_pool, deposits.clone().collect())
            .await
            .unwrap()
    };
    let apply_deposits = L1Action {
        context: L1ActionContext::SubmitBlock {
            l2block: block_result.block.clone(),
            deposit_requests: deposits.collect(),
            deposit_asset_scripts: Default::default(),
        },
        transaction: build_sync_tx(rollup_cell, block_result),
        l2block_committed_info: L2BlockCommittedInfo::new_builder()
            .number(1u64.pack())
            .build(),
    };
    let param = SyncParam {
        updates: vec![apply_deposits],
        reverts: Default::default(),
    };
    chain.sync(param).await.unwrap();
    assert!(chain.last_sync_event().is_success());

    // Generate random withdrawals, deposits, txs
    const WITHDRAWAL_CAPACITY: u64 = 1000 * CKB;
    let withdrawal_count = rand::random::<u8>() % 10 + 5;
    let random_withdrawals: Vec<_> = {
        let withdrawal_accounts = accounts.iter().take(withdrawal_count as usize);
        withdrawal_accounts
            .map(|account_script| {
                let raw = RawWithdrawalRequest::new_builder()
                    .capacity(WITHDRAWAL_CAPACITY.pack())
                    .account_script_hash(account_script.hash().pack())
                    .sudt_script_hash(H256::zero().pack())
                    .build();
                WithdrawalRequest::new_builder().raw(raw).build()
            })
            .collect()
    };

    let random_deposits: Vec<_> = {
        let count = rand::random::<u8>() % 20 + 5;
        let users = (0..count).map(|_| random_always_success_script(&rollup_script_hash));
        let deposits = users.map(|user_script| {
            DepositRequest::new_builder()
                .capacity(DEPOSIT_CAPACITY.pack())
                .sudt_script_hash(H256::zero().pack())
                .amount(0.pack())
                .script(user_script)
                .build()
        });

        let rollup_context = chain.generator().rollup_context();
        deposits
            .map(|r| into_deposit_info_cell(rollup_context, r))
            .collect()
    };

    let random_txs: Vec<_> = {
        let tx_accounts = accounts.iter().skip(withdrawal_count as usize);
        let db = chain.store().begin_transaction();
        let state = db.state_tree(StateContext::ReadOnly).unwrap();
        tx_accounts
            .map(|account_script| {
                let from_id = state
                    .get_account_id_by_script_hash(&account_script.hash().into())
                    .unwrap();
                let to_script = random_always_success_script(&rollup_script_hash);
                let transfer = SUDTTransfer::new_builder()
                    .amount((DEPOSIT_CAPACITY as u128 / 2).pack())
                    .to(to_short_address(&to_script.hash().into()).pack())
                    .build();
                let args = SUDTArgs::new_builder().set(transfer).build();
                let raw = RawL2Transaction::new_builder()
                    .from_id(from_id.unwrap().pack())
                    .to_id(1u32.pack()) // 1 is reserved for sudt
                    .args(args.as_bytes().pack())
                    .build();
                L2Transaction::new_builder().raw(raw).build()
            })
            .collect()
    };

    // Push withdrawals, deposits and txs
    let finalized_custodians = CollectedCustodianCells {
        capacity: ((withdrawal_count + 2) as u64 * WITHDRAWAL_CAPACITY) as u128,
        cells_info: vec![Default::default()],
        ..Default::default()
    };
    {
        let mem_pool = chain.mem_pool().as_ref().unwrap();
        let mut mem_pool = mem_pool.lock().await;
        let provider = DummyMemPoolProvider {
            deposit_cells: random_deposits.clone(),
            fake_blocktime: Duration::from_millis(0),
            collected_custodians: finalized_custodians.clone(),
        };
        mem_pool.set_provider(Box::new(provider));
        for withdrawal in random_withdrawals.clone() {
            mem_pool
                .push_withdrawal_request(withdrawal.into())
                .await
                .unwrap();
        }
        mem_pool.reset_mem_block().await.unwrap();
        for tx in random_txs.clone() {
            mem_pool.push_transaction(tx).await.unwrap();
        }

        let mem_block = mem_pool.mem_block();
        assert_eq!(mem_block.withdrawals().len(), random_withdrawals.len());
        assert_eq!(mem_block.deposits().len(), random_deposits.len());
        assert_eq!(mem_block.txs().len(), random_txs.len());

        // Dump mem block and refresh deposits
        mem_pool.save_mem_block().unwrap();
    }

    // Simualte chain restart and restore mem block
    let provider = DummyMemPoolProvider {
        deposit_cells: vec![], // IMPORTANT: Remove deposits, previous deposits in mem block should be recovered and used
        fake_blocktime: Duration::from_millis(0),
        collected_custodians: finalized_custodians,
    };
    let chain = restart_chain(&chain, rollup_type_script.clone(), Some(provider)).await;
    {
        let mem_pool = chain.mem_pool().as_ref().unwrap();
        let mut mem_pool = mem_pool.lock().await;

        let mem_block = mem_pool.mem_block();
        assert_eq!(mem_block.withdrawals().len(), random_withdrawals.len());
        assert_eq!(mem_block.deposits().len(), random_deposits.len());
        assert!(mem_block.txs().is_empty());
        assert_eq!(
            mem_block.state_checkpoints().len(),
            random_withdrawals.len()
        );
        assert_eq!(
            mem_pool.pending_restored_tx_hashes().len(),
            random_txs.len()
        );
    }

    let _rpc_server = {
        let store = chain.store().clone();
        let mem_pool = chain.mem_pool().clone();
        let generator = chain_generator(&chain, rollup_type_script.clone());
        let rollup_config = generator.rollup_context().rollup_config.to_owned();
        let rollup_context = generator.rollup_context().to_owned();
        let rpc_client = {
            let indexer_client =
                CKBIndexerClient::with_url(&RPCClientConfig::default().indexer_url).unwrap();
            let ckb_client = CKBClient::with_url(&RPCClientConfig::default().ckb_url).unwrap();
            let rollup_type_script =
                ckb_types::packed::Script::new_unchecked(rollup_type_script.as_bytes());
            RPCClient::new(
                rollup_type_script,
                rollup_context,
                ckb_client,
                indexer_client,
            )
        };
        let dynamic_config_manager =
            Arc::new(ArcSwap::from_pointee(DynamicConfigManager::default()));
        let args: RegistryArgs<TestModeControl> = RegistryArgs {
            store,
            mem_pool,
            generator,
            tests_rpc_impl: None,
            rollup_config,
            mem_pool_config: Default::default(),
            node_mode: Default::default(),
            rpc_client,
            send_tx_rate_limit: Default::default(),
            server_config: Default::default(),
            dynamic_config_manager,
            last_submitted_tx_hash: None,
            withdrawal_to_v1_config: None,
        };

        Registry::create(args).await
    };

    // Check restore withdrawals, deposits and txs
    {
        let mut count = 10;
        while count > 0 {
            {
                let mem_pool = chain.mem_pool().as_ref().unwrap();
                let mut mem_pool = mem_pool.lock().await;

                if mem_pool.pending_restored_tx_hashes().is_empty() {
                    // Restored txs are processed
                    break;
                }
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
            count -= 1;
        }

        let mem_pool = chain.mem_pool().as_ref().unwrap();
        let mut mem_pool = mem_pool.lock().await;
        if count == 0 && !mem_pool.pending_restored_tx_hashes().is_empty() {
            panic!("mem block restored txs aren't reinjected");
        }

        let mem_block = mem_pool.mem_block();
        assert_eq!(mem_block.withdrawals().len(), random_withdrawals.len());
        assert_eq!(mem_block.deposits().len(), random_deposits.len());
        assert_eq!(mem_block.txs().len(), random_txs.len());
        assert_eq!(
            mem_block.state_checkpoints().len(),
            random_withdrawals.len() + random_txs.len()
        );

        // Also check txs order
        for (reinjected_tx_hash, tx) in mem_block.txs().iter().zip(random_txs.iter()) {
            assert_eq!(reinjected_tx_hash.pack(), tx.hash().pack());
        }
    }
}

fn random_always_success_script(rollup_script_hash: &H256) -> Script {
    let random_bytes: [u8; 32] = rand::random();
    Script::new_builder()
        .code_hash(ALWAYS_SUCCESS_CODE_HASH.clone().pack())
        .hash_type(ScriptHashType::Type.into())
        .args({
            let mut args = rollup_script_hash.as_slice().to_vec();
            args.extend_from_slice(&random_bytes);
            args.pack()
        })
        .build()
}

fn into_deposit_info_cell(rollup_context: &RollupContext, request: DepositRequest) -> DepositInfo {
    let rollup_script_hash = rollup_context.rollup_script_hash;
    let deposit_lock_type_hash = rollup_context.rollup_config.deposit_script_type_hash();

    let lock_args = {
        let cancel_timeout = 0xc0000000000004b0u64;
        let mut buf: Vec<u8> = Vec::new();
        let deposit_args = DepositLockArgs::new_builder()
            .cancel_timeout(cancel_timeout.pack())
            .build();
        buf.extend(rollup_script_hash.as_slice());
        buf.extend(deposit_args.as_slice());
        buf
    };

    let out_point = OutPoint::new_builder()
        .tx_hash(rand::random::<[u8; 32]>().pack())
        .build();
    let lock_script = Script::new_builder()
        .code_hash(deposit_lock_type_hash)
        .hash_type(ScriptHashType::Type.into())
        .args(lock_args.pack())
        .build();
    let output = CellOutput::new_builder()
        .lock(lock_script)
        .capacity(request.capacity())
        .build();

    let cell = CellInfo {
        out_point,
        output,
        data: request.amount().as_bytes(),
    };

    DepositInfo { cell, request }
}
