#![allow(clippy::mutable_key_type)]

use std::collections::{HashMap, HashSet};
use std::iter::FromIterator;
use std::time::Duration;

use crate::testing_tool::chain::{
    build_sync_tx, construct_block, setup_chain, ALWAYS_SUCCESS_CODE_HASH, ALWAYS_SUCCESS_PROGRAM,
    WITHDRAWAL_LOCK_PROGRAM,
};
use crate::testing_tool::mem_pool_provider::DummyMemPoolProvider;
use crate::testing_tool::verify_tx::{verify_tx, TxWithContext};

use async_trait::async_trait;
use ckb_types::prelude::{Builder, Entity};
use gw_block_producer::withdrawal_unlocker::BuildUnlockWithdrawalToOwner;
use gw_chain::chain::{L1Action, L1ActionContext, SyncParam};
use gw_common::H256;
use gw_config::BlockProducerConfig;
use gw_types::core::ScriptHashType;
use gw_types::offchain::{CellInfo, CollectedCustodianCells, InputCellInfo, RollupContext};
use gw_types::packed::{
    CellDep, CellInput, CellOutput, DepositRequest, GlobalState, L2BlockCommittedInfo, OutPoint,
    RawWithdrawalRequest, RollupConfig, Script, WithdrawalRequest, WithdrawalRequestExtra,
};
use gw_types::prelude::{Pack, Unpack};
use gw_utils::transaction_skeleton::TransactionSkeleton;

const CKB: u64 = 100000000;
const MAX_MEM_BLOCK_WITHDRAWALS: u8 = 50;

#[test]
fn test_build_unlock_to_owner_tx() {
    let _ = env_logger::builder().is_test(true).try_init();

    let last_finalized_block_number = 100u64;
    let global_state = GlobalState::new_builder()
        .last_finalized_block_number(last_finalized_block_number.pack())
        .build();

    let rollup_type = random_always_success_script(None);
    let rollup_script_hash: H256 = rollup_type.hash().into();
    let rollup_cell = CellInfo {
        data: global_state.as_bytes(),
        out_point: OutPoint::new_builder()
            .tx_hash(rand::random::<[u8; 32]>().pack())
            .build(),
        output: CellOutput::new_builder()
            .type_(Some(rollup_type.clone()).pack())
            .build(),
    };
    let mut chain = setup_chain(rollup_type.clone());

    let always_type = random_always_success_script(None);
    let always_cell = CellInfo {
        out_point: OutPoint::new_builder()
            .tx_hash(rand::random::<[u8; 32]>().pack())
            .build(),
        output: CellOutput::new_builder()
            .capacity((1000 * 10u64.pow(8)).pack())
            .type_(Some(always_type.clone()).pack())
            .build(),
        data: ALWAYS_SUCCESS_PROGRAM.clone(),
    };

    let sudt_script = Script::new_builder()
        .code_hash(always_type.hash().pack())
        .hash_type(ScriptHashType::Type.into())
        .args(vec![rand::random::<u8>(), 32].pack())
        .build();

    let withdrawal_lock_type = random_always_success_script(None);
    let withdrawal_lock_cell = CellInfo {
        out_point: OutPoint::new_builder()
            .tx_hash(rand::random::<[u8; 32]>().pack())
            .build(),
        output: CellOutput::new_builder()
            .capacity((1000 * 10u64.pow(8)).pack())
            .type_(Some(withdrawal_lock_type.clone()).pack())
            .build(),
        data: WITHDRAWAL_LOCK_PROGRAM.clone(),
    };

    let rollup_context = RollupContext {
        rollup_script_hash: rollup_type.hash().into(),
        rollup_config: RollupConfig::new_builder()
            .withdrawal_script_type_hash(withdrawal_lock_type.hash().pack())
            .l1_sudt_script_type_hash(always_type.hash().pack())
            .finality_blocks(1u64.pack())
            .build(),
    };

    let block_producer_config = BlockProducerConfig {
        withdrawal_cell_lock_dep: CellDep::new_builder()
            .out_point(withdrawal_lock_cell.out_point.clone())
            .build()
            .into(),
        l1_sudt_type_dep: CellDep::new_builder()
            .out_point(always_cell.out_point.clone())
            .build()
            .into(),
        ..Default::default()
    };

    // Deposit random accounts
    const DEPOSIT_CAPACITY: u64 = 1000000 * CKB;
    const DEPOSIT_AMOUNT: u128 = 1000;
    let account_count = (rand::random::<u8>() % 100 + 5) % MAX_MEM_BLOCK_WITHDRAWALS;
    let accounts: Vec<_> = (0..account_count)
        .map(|_| random_always_success_script(Some(&rollup_script_hash)))
        .collect();
    let deposits = accounts.iter().map(|account_script| {
        DepositRequest::new_builder()
            .capacity(DEPOSIT_CAPACITY.pack())
            .sudt_script_hash(sudt_script.hash().pack())
            .amount(DEPOSIT_AMOUNT.pack())
            .script(account_script.to_owned())
            .build()
    });

    let block_result = {
        let mem_pool = chain.mem_pool().as_ref().unwrap();
        let mut mem_pool = smol::block_on(mem_pool.lock());
        construct_block(&chain, &mut mem_pool, deposits.clone().collect()).unwrap()
    };
    let apply_deposits = L1Action {
        context: L1ActionContext::SubmitBlock {
            l2block: block_result.block.clone(),
            deposit_requests: deposits.collect(),
            deposit_asset_scripts: Default::default(),
        },
        transaction: build_sync_tx(rollup_cell.output.clone(), block_result),
        l2block_committed_info: L2BlockCommittedInfo::new_builder()
            .number(1u64.pack())
            .build(),
    };
    let param = SyncParam {
        updates: vec![apply_deposits],
        reverts: Default::default(),
    };
    chain.sync(param).unwrap();
    assert!(chain.last_sync_event().is_success());

    // Generate random withdrawals(w/wo owner lock)
    const WITHDRAWAL_CAPACITY: u64 = 1000 * CKB;
    const WITHDRAWAL_AMOUNT: u128 = 100;
    let no_owner_lock_count = rand::random::<u8>() % (accounts.len() as u8 / 2) + 1;
    let withdrawals_no_lock = {
        let accounts = accounts.iter().take(no_owner_lock_count as usize);
        accounts.map(|account_script| {
            let raw = RawWithdrawalRequest::new_builder()
                .capacity(WITHDRAWAL_CAPACITY.pack())
                .amount(WITHDRAWAL_AMOUNT.pack())
                .account_script_hash(account_script.hash().pack())
                .owner_lock_hash(account_script.hash().pack())
                .sudt_script_hash(sudt_script.hash().pack())
                .build();
            WithdrawalRequest::new_builder().raw(raw).build().into()
        })
    };
    let withdrawals_lock = {
        let accounts = accounts.iter().skip(no_owner_lock_count as usize);
        accounts.map(|account_script| {
            let raw = RawWithdrawalRequest::new_builder()
                .capacity(WITHDRAWAL_CAPACITY.pack())
                .amount(WITHDRAWAL_AMOUNT.pack())
                .account_script_hash(account_script.hash().pack())
                .owner_lock_hash(account_script.hash().pack())
                .sudt_script_hash(sudt_script.hash().pack())
                .build();
            let req = WithdrawalRequest::new_builder().raw(raw).build();
            WithdrawalRequestExtra::new_builder()
                .request(req)
                .owner_lock(Some(account_script.to_owned()).pack())
                .build()
        })
    };

    // Push withdrawals
    let finalized_custodians = CollectedCustodianCells {
        capacity: ((accounts.len() as u128 + 1) * WITHDRAWAL_CAPACITY as u128),
        cells_info: vec![Default::default()],
        sudt: HashMap::from_iter([(
            sudt_script.hash(),
            (
                WITHDRAWAL_AMOUNT * (accounts.len() as u128 + 1),
                sudt_script.clone(),
            ),
        )]),
    };

    {
        let mem_pool = chain.mem_pool().as_ref().unwrap();
        let mut mem_pool = smol::block_on(mem_pool.lock());
        let provider = DummyMemPoolProvider {
            deposit_cells: vec![],
            fake_blocktime: Duration::from_millis(0),
            collected_custodians: finalized_custodians.clone(),
        };
        mem_pool.set_provider(Box::new(provider));

        for withdrawal in withdrawals_no_lock.chain(withdrawals_lock) {
            mem_pool.push_withdrawal_request(withdrawal).unwrap();
        }

        mem_pool.reset_mem_block().unwrap();
        assert_eq!(mem_pool.mem_block().withdrawals().len(), accounts.len());
    }

    let block_result = {
        let mem_pool = chain.mem_pool().as_ref().unwrap();
        let mut mem_pool = smol::block_on(mem_pool.lock());
        construct_block(&chain, &mut mem_pool, vec![]).unwrap()
    };
    assert_eq!(block_result.block.withdrawals().len(), accounts.len());

    // Generate withdrawal cells
    let withdrawal_extras = block_result
        .withdrawal_extras
        .into_iter()
        .map(|w| (w.hash().into(), w));
    let generated_withdrawals = gw_block_producer::withdrawal::generate(
        &rollup_context,
        finalized_custodians,
        &block_result.block,
        &block_producer_config,
        &withdrawal_extras.collect(),
    )
    .expect("generate")
    .expect("some withdrawals cell");

    let random_withdrawal_cells = {
        let outputs = generated_withdrawals.outputs.into_iter();
        outputs
            .map(|(output, data)| CellInfo {
                output,
                data,
                out_point: OutPoint::new_builder()
                    .tx_hash(rand::random::<[u8; 32]>().pack())
                    .build(),
            })
            .collect::<Vec<_>>()
    };

    // Check unlock to owner tx
    let mut unlocker = DummyUnlocker {
        rollup_cell: rollup_cell.clone(),
        rollup_context: rollup_context.clone(),
        block_producer_config,
        withdrawals: random_withdrawal_cells.clone(),
    };
    let cell_deps = vec![
        into_input_cell(rollup_cell),
        into_input_cell(always_cell),
        into_input_cell(withdrawal_lock_cell),
    ];

    let unlocked = Default::default();
    let (tx, to_unlock) = smol::block_on(unlocker.query_and_unlock_to_owner(&unlocked))
        .expect("unlock")
        .expect("skip no owner lock");
    assert_eq!(
        to_unlock.len(),
        accounts.len() - no_owner_lock_count as usize
    );

    let inputs = {
        let cells = random_withdrawal_cells.clone().into_iter();
        cells.map(into_input_cell).collect()
    };
    let tx_with_context = TxWithContext {
        tx,
        cell_deps: cell_deps.clone(),
        inputs,
    };

    verify_tx(tx_with_context, 7000_0000u64).expect("pass");

    // Simulate rpc client filter no owner lock withdrawal cells
    let last_finalized_block_number = block_result.block.raw().number().unpack();
    let unlockable_random_withdrawals: Vec<_> = random_withdrawal_cells
        .into_iter()
        .filter(|cell| {
            gw_rpc_client::withdrawal::verify_unlockable_to_owner(
                cell,
                last_finalized_block_number,
                &rollup_context.rollup_config.l1_sudt_script_type_hash(),
            )
            .is_ok()
        })
        .collect();
    assert_eq!(
        unlockable_random_withdrawals.len(),
        accounts.len() - no_owner_lock_count as usize
    );

    unlocker.withdrawals = unlockable_random_withdrawals.clone();
    let (tx, _to_unlock) = smol::block_on(unlocker.query_and_unlock_to_owner(&unlocked))
        .expect("unlock")
        .expect("some withdrawals tx");

    let inputs = unlockable_random_withdrawals
        .into_iter()
        .map(into_input_cell)
        .collect();

    let tx_with_context = TxWithContext {
        tx,
        cell_deps,
        inputs,
    };

    verify_tx(tx_with_context, 7000_0000u64).expect("pass");
}

struct DummyUnlocker {
    rollup_cell: CellInfo,
    rollup_context: RollupContext,
    block_producer_config: BlockProducerConfig,
    withdrawals: Vec<CellInfo>,
}

#[async_trait]
impl BuildUnlockWithdrawalToOwner for DummyUnlocker {
    fn rollup_context(&self) -> &RollupContext {
        &self.rollup_context
    }

    fn block_producer_config(&self) -> &BlockProducerConfig {
        &self.block_producer_config
    }

    async fn query_rollup_cell(&self) -> anyhow::Result<Option<CellInfo>> {
        Ok(Some(self.rollup_cell.clone()))
    }

    async fn query_unlockable_withdrawals(
        &self,
        _last_finalized_block_number: u64,
        _unlocked: &HashSet<OutPoint>,
    ) -> anyhow::Result<Vec<CellInfo>> {
        Ok(self.withdrawals.clone())
    }

    async fn complete_tx(
        &self,
        tx_skeleton: TransactionSkeleton,
    ) -> anyhow::Result<gw_types::packed::Transaction> {
        Ok(tx_skeleton.seal(&[], vec![])?.transaction)
    }
}

fn into_input_cell(cell: CellInfo) -> InputCellInfo {
    InputCellInfo {
        input: CellInput::new_builder()
            .previous_output(cell.out_point.clone())
            .build(),
        cell,
    }
}

fn random_always_success_script(opt_rollup_script_hash: Option<&H256>) -> Script {
    let random_bytes: [u8; 32] = rand::random();
    Script::new_builder()
        .code_hash(ALWAYS_SUCCESS_CODE_HASH.clone().pack())
        .hash_type(ScriptHashType::Type.into())
        .args({
            let mut args = opt_rollup_script_hash
                .map(|h| h.as_slice().to_vec())
                .unwrap_or_default();
            args.extend_from_slice(&random_bytes);
            args.pack()
        })
        .build()
}
