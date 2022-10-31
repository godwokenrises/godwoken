use std::time::Duration;

use crate::testing_tool::chain::{
    apply_block_result, construct_block, into_deposit_info_cell, produce_empty_block,
    restart_chain, setup_chain, DEFAULT_FINALITY_BLOCKS, TEST_CHAIN_ID,
};
use crate::testing_tool::common::random_always_success_script;
use crate::testing_tool::mem_pool_provider::DummyMemPoolProvider;
use crate::testing_tool::rpc_server::RPCServer;

use ckb_types::prelude::{Builder, Entity};
use ckb_vm::Bytes;
use gw_common::builtins::ETH_REGISTRY_ACCOUNT_ID;
use gw_common::registry_address::RegistryAddress;
use gw_common::{state::State, H256};
use gw_rpc_server::registry::Registry;
use gw_store::state::history::history_state::RWConfig;
use gw_store::state::BlockStateDB;
use gw_types::packed::{
    DepositInfoVec, DepositRequest, Fee, L2Transaction, RawL2Transaction, RawWithdrawalRequest,
    SUDTArgs, SUDTTransfer, Script, WithdrawalRequest, WithdrawalRequestExtra,
};
use gw_types::prelude::*;
use gw_types::U256;
use gw_utils::local_cells::LocalCellsManager;

const CKB: u64 = 100000000;

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_restore_mem_block() {
    let _ = env_logger::builder().is_test(true).try_init();

    let rollup_type_script = Script::default();
    let rollup_script_hash: H256 = rollup_type_script.hash().into();
    let mut chain = setup_chain(rollup_type_script.clone()).await;
    let rollup_context = chain.generator().rollup_context();

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
    let deposit_info_vec = DepositInfoVec::new_builder()
        .extend(deposits.map(|d| into_deposit_info_cell(rollup_context, d).pack()))
        .build();

    let block_result = {
        let mem_pool = chain.mem_pool().as_ref().unwrap();
        let mut mem_pool = mem_pool.lock().await;
        construct_block(&chain, &mut mem_pool, deposit_info_vec.clone())
            .await
            .unwrap()
    };
    apply_block_result(
        &mut chain,
        block_result,
        deposit_info_vec,
        Default::default(),
    )
    .await;

    for _ in 0..DEFAULT_FINALITY_BLOCKS {
        produce_empty_block(&mut chain).await.unwrap();
    }

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
                    .chain_id(TEST_CHAIN_ID.pack())
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
        let state = BlockStateDB::from_store(&db, RWConfig::readonly()).unwrap();
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
                    .chain_id(crate::testing_tool::chain::TEST_CHAIN_ID.pack())
                    .build();
                L2Transaction::new_builder().raw(raw).build()
            })
            .collect()
    };

    // Push withdrawals, deposits and txs
    {
        let mem_pool = chain.mem_pool().as_ref().unwrap();
        let mut mem_pool = mem_pool.lock().await;
        let provider = DummyMemPoolProvider {
            deposit_cells: random_deposits.clone(),
            fake_blocktime: Duration::from_millis(0),
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
            mem_pool.push_transaction(tx).unwrap();
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
    };
    let chain = restart_chain(&chain, rollup_type_script.clone(), Some(provider)).await;
    chain.notify_new_tip().await.unwrap();
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
        let args = RPCServer::default_registry_args(&chain, rollup_type_script.clone(), None);
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
