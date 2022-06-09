use std::sync::Arc;
use std::time::Duration;

use crate::testing_tool::chain::{
    apply_block_result, chain_generator, construct_block, restart_chain, setup_chain,
};
use crate::testing_tool::common::random_always_success_script;
use crate::testing_tool::mem_pool_provider::DummyMemPoolProvider;

use ckb_types::prelude::{Builder, Entity};
use ckb_vm::Bytes;
use gw_block_producer::test_mode_control::TestModeControl;
use gw_common::builtins::ETH_REGISTRY_ACCOUNT_ID;
use gw_common::registry_address::RegistryAddress;
use gw_common::{state::State, H256};
use gw_config::RPCClientConfig;
use gw_dynamic_config::manager::DynamicConfigManager;
use gw_generator::ArcSwap;
use gw_rpc_client::ckb_client::CKBClient;
use gw_rpc_client::indexer_client::CKBIndexerClient;
use gw_rpc_client::rpc_client::RPCClient;
use gw_rpc_server::registry::{Registry, RegistryArgs};
use gw_store::state::state_db::StateContext;
use gw_types::core::ScriptHashType;
use gw_types::offchain::{CellInfo, DepositInfo, FinalizedCustodianCapacity, RollupContext};
use gw_types::packed::{
    CellOutput, DepositLockArgs, DepositRequest, Fee, L2Transaction, OutPoint, RawL2Transaction,
    RawWithdrawalRequest, SUDTArgs, SUDTTransfer, Script, WithdrawalRequest,
    WithdrawalRequestExtra,
};
use gw_types::prelude::Pack;
use gw_types::U256;
use gw_utils::local_cells::LocalCellsManager;

const CKB: u64 = 100000000;

#[tokio::test(flavor = "multi_thread")]
async fn test_restore_mem_block() {
    let _ = env_logger::builder().is_test(true).try_init();

    let rollup_type_script = Script::default();
    let rollup_script_hash: H256 = rollup_type_script.hash().into();
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
            .registry_id(gw_common::builtins::ETH_REGISTRY_ACCOUNT_ID.pack())
            .build()
    });

    let block_result = {
        let mem_pool = chain.mem_pool().as_ref().unwrap();
        let mut mem_pool = mem_pool.lock().await;
        construct_block(&chain, &mut mem_pool, deposits.clone().collect())
            .await
            .unwrap()
    };
    apply_block_result(
        &mut chain,
        block_result,
        deposits.collect(),
        Default::default(),
    )
    .await;

    // Generate random withdrawals, deposits, txs
    const WITHDRAWAL_CAPACITY: u64 = 1000 * CKB;
    let withdrawal_count = rand::random::<u8>() % 10 + 5;
    let random_withdrawals: Vec<_> = {
        let withdrawal_accounts = accounts.iter().take(withdrawal_count as usize);
        withdrawal_accounts
            .map(|account_script| {
                let owner_lock = Script::default();
                let raw = RawWithdrawalRequest::new_builder()
                    .capacity(WITHDRAWAL_CAPACITY.pack())
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
                .registry_id(gw_common::builtins::ETH_REGISTRY_ACCOUNT_ID.pack())
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
                let to_addr =
                    RegistryAddress::new(ETH_REGISTRY_ACCOUNT_ID, to_script.hash()[0..20].to_vec());
                let transfer = SUDTTransfer::new_builder()
                    .amount(U256::from(DEPOSIT_CAPACITY as u128 / 2).pack())
                    .to_address(Bytes::from(to_addr.to_bytes()).pack())
                    .fee(
                        Fee::new_builder()
                            .registry_id(gw_common::builtins::ETH_REGISTRY_ACCOUNT_ID.pack())
                            .build(),
                    )
                    .build();
                let args = SUDTArgs::new_builder().set(transfer).build();
                let raw = RawL2Transaction::new_builder()
                    .from_id(from_id.unwrap().pack())
                    .to_id(gw_common::builtins::CKB_SUDT_ACCOUNT_ID.pack()) // 1 is reserved for sudt
                    .args(args.as_bytes().pack())
                    .build();
                L2Transaction::new_builder().raw(raw).build()
            })
            .collect()
    };

    // Push withdrawals, deposits and txs
    let finalized_custodians = FinalizedCustodianCapacity {
        capacity: ((withdrawal_count + 2) as u64 * WITHDRAWAL_CAPACITY) as u128,
        ..Default::default()
    };
    {
        let mem_pool = chain.mem_pool().as_ref().unwrap();
        let mut mem_pool = mem_pool.lock().await;
        let provider = DummyMemPoolProvider {
            deposit_cells: random_deposits.clone(),
            fake_blocktime: Duration::from_millis(0),
            deposit_custodians: finalized_custodians.clone(),
        };
        mem_pool.set_provider(Box::new(provider));
        for withdrawal in random_withdrawals.clone() {
            mem_pool.push_withdrawal_request(withdrawal).await.unwrap();
        }
        mem_pool
            .reset_mem_block(&LocalCellsManager::default())
            .await
            .unwrap();
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
        deposit_custodians: finalized_custodians,
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
            chain_config: Default::default(),
            consensus_config: Default::default(),
            dynamic_config_manager,
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
