use std::collections::HashMap;
use std::time::Duration;

use crate::testing_tool::chain::{
    build_sync_tx, construct_block, construct_block_with_timestamp, restart_chain, setup_chain,
};
use crate::testing_tool::common::random_always_success_script;
use crate::testing_tool::mem_pool_provider::DummyMemPoolProvider;

use ckb_types::prelude::{Builder, Entity};
use gw_chain::chain::{L1Action, L1ActionContext, SyncParam};
use gw_common::H256;
use gw_types::offchain::CollectedCustodianCells;
use gw_types::packed::{
    CellOutput, DepositRequest, L2BlockCommittedInfo, RawWithdrawalRequest, Script,
    WithdrawalRequest, WithdrawalRequestExtra,
};
use gw_types::prelude::Pack;

const ACCOUNTS_COUNT: usize = 20;
const CKB: u64 = 100000000;
const DEPOSIT_CAPACITY: u64 = 1000000 * CKB;
const WITHDRAWAL_CAPACITY: u64 = 1000 * CKB;

#[tokio::test]
async fn test_restore_mem_pool_pending_withdrawal() {
    let _ = env_logger::builder().is_test(true).try_init();

    let rollup_type_script = Script::default();
    let rollup_script_hash: H256 = rollup_type_script.hash().into();
    let rollup_cell = CellOutput::new_builder()
        .type_(Some(rollup_type_script.clone()).pack())
        .build();
    let mut chain = setup_chain(rollup_type_script.clone()).await;

    // Deposit accounts
    let accounts: Vec<_> = (0..ACCOUNTS_COUNT)
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
    let apply_deposits = L1Action {
        context: L1ActionContext::SubmitBlock {
            l2block: block_result.block.clone(),
            deposit_requests: deposits.collect(),
            deposit_asset_scripts: Default::default(),
            withdrawals: Default::default(),
        },
        transaction: build_sync_tx(rollup_cell.clone(), block_result),
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

    // Generate withdrawals
    let mut withdrawals: Vec<_> = {
        accounts
            .iter()
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
    let pending_withdrawals = withdrawals.split_off(withdrawals.len() - 10);
    let mem_block_withdrawals = withdrawals;
    assert!(!pending_withdrawals.is_empty());

    // Insert error nonce withdrawal and expect them to be removed during pending restore
    let invalid_withdrawals_count = pending_withdrawals.len() - 5;
    assert!(invalid_withdrawals_count > 0);
    let invalid_withdrawals: Vec<_> = pending_withdrawals
        .iter()
        .take(invalid_withdrawals_count)
        .map(|w| {
            let raw = w.request().raw();
            let raw_with_invalid_nonce = raw.as_builder().nonce(9u32.pack()).build();
            let request = w.request().as_builder().raw(raw_with_invalid_nonce).build();
            w.clone().as_builder().request(request).build()
        })
        .collect();
    {
        let db = chain.store().begin_transaction();
        for withdrawal in invalid_withdrawals {
            db.insert_mem_pool_withdrawal(&withdrawal.hash().into(), withdrawal)
                .unwrap();
        }
        db.commit().unwrap();
    }

    // Push withdrawals
    let finalized_custodians = CollectedCustodianCells {
        capacity: ((ACCOUNTS_COUNT + 2) as u64 * WITHDRAWAL_CAPACITY) as u128,
        cells_info: vec![Default::default()],
        ..Default::default()
    };
    {
        let mem_pool = chain.mem_pool().as_ref().unwrap();
        let mut mem_pool = mem_pool.lock().await;
        let provider = DummyMemPoolProvider {
            deposit_cells: vec![],
            fake_blocktime: Duration::from_millis(0),
            collected_custodians: finalized_custodians.clone(),
        };
        mem_pool.set_provider(Box::new(provider));

        for withdrawal in mem_block_withdrawals.clone() {
            mem_pool.push_withdrawal_request(withdrawal).await.unwrap();
        }
        mem_pool.reset_mem_block().await.unwrap();

        for withdrawal in pending_withdrawals.clone() {
            mem_pool.push_withdrawal_request(withdrawal).await.unwrap();
        }

        let mem_block = mem_pool.mem_block();
        assert_eq!(mem_block.withdrawals().len(), mem_block_withdrawals.len());

        // Dump mem block
        mem_pool.save_mem_block().unwrap();

        let db = chain.store().begin_transaction();
        assert_eq!(
            db.get_mem_pool_withdrawal_iter().count(),
            mem_block_withdrawals.len() + pending_withdrawals.len() + invalid_withdrawals_count
        );
    }

    // Simualte chain restart
    let provider = DummyMemPoolProvider {
        deposit_cells: vec![],
        fake_blocktime: Duration::from_millis(0),
        collected_custodians: finalized_custodians,
    };
    let mut chain = restart_chain(&chain, rollup_type_script.clone(), Some(provider)).await;

    // Check restore mem block withdrawals
    {
        let mem_pool = chain.mem_pool().as_ref().unwrap();
        let mem_pool = mem_pool.lock().await;

        let mem_block = mem_pool.mem_block();
        assert_eq!(mem_block.withdrawals().len(), mem_block_withdrawals.len());
        assert_eq!(
            mem_block.state_checkpoints().len(),
            mem_block_withdrawals.len()
        );
    }

    // Check whether invalid withdrawals are removed
    let db = chain.store().begin_transaction();
    assert_eq!(
        db.get_mem_pool_withdrawal_iter().count(),
        mem_block_withdrawals.len() + pending_withdrawals.len()
    );

    // Produce new block then check pending withdrawals
    let block_result = {
        let mem_pool = chain.mem_pool().as_ref().unwrap();
        let mut mem_pool = mem_pool.lock().await;
        construct_block_with_timestamp(&chain, &mut mem_pool, vec![], 0, false)
            .await
            .unwrap()
    };
    let block_withdrawals: Vec<_> = {
        let withdrawals: HashMap<_, _> = mem_block_withdrawals
            .into_iter()
            .map(|w| (w.hash(), w))
            .collect();

        block_result
            .block
            .withdrawals()
            .into_iter()
            .map(|w| withdrawals.get(&w.hash()).unwrap().clone())
            .collect()
    };
    let apply_mem_block_withdrawals = L1Action {
        context: L1ActionContext::SubmitBlock {
            l2block: block_result.block.clone(),
            deposit_requests: vec![],
            deposit_asset_scripts: Default::default(),
            withdrawals: block_withdrawals,
        },
        transaction: build_sync_tx(rollup_cell, block_result),
        l2block_committed_info: L2BlockCommittedInfo::new_builder()
            .number(2u64.pack())
            .build(),
    };
    let param = SyncParam {
        updates: vec![apply_mem_block_withdrawals],
        reverts: Default::default(),
    };
    chain.sync(param).await.unwrap();
    assert!(chain.last_sync_event().is_success());

    let mem_pool = chain.mem_pool().as_ref().unwrap();
    let mut mem_pool = mem_pool.lock().await;
    mem_pool.reset_mem_block().await.unwrap();

    let mem_block = mem_pool.mem_block();
    assert_eq!(mem_block.withdrawals().len(), pending_withdrawals.len());
    assert_eq!(
        mem_block.state_checkpoints().len(),
        pending_withdrawals.len()
    );
}
