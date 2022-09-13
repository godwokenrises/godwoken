#![allow(clippy::mutable_key_type)]

use std::collections::HashMap;
use std::iter::FromIterator;
use std::sync::Arc;
use std::time::Duration;

use crate::testing_tool::chain::{
    construct_block_with_timestamp, TestChain, ALWAYS_SUCCESS_CODE_HASH, ALWAYS_SUCCESS_PROGRAM,
    CUSTODIAN_LOCK_PROGRAM, STAKE_LOCK_PROGRAM, STATE_VALIDATOR_TYPE_PROGRAM,
};
use crate::testing_tool::mem_pool_provider::DummyMemPoolProvider;
use crate::testing_tool::verify_tx::{verify_tx, TxWithContext};

use anyhow::Result;
use async_trait::async_trait;
use ckb_types::prelude::{Builder, Entity};
use gw_block_producer::withdrawal::BlockWithdrawals;
use gw_block_producer::withdrawal_finalizer::FinalizeWithdrawalToOwner;
use gw_block_producer::withdrawal_unlocker::Guard;
use gw_common::smt::generate_block_proof;
use gw_common::sparse_merkle_tree::CompiledMerkleProof;
use gw_common::H256;
use gw_config::ContractsCellDep;
use gw_store::traits::chain_store::ChainStore;
use gw_store::Store;
use gw_types::bytes::Bytes;
use gw_types::core::{AllowedEoaType, DepType, ScriptHashType};
use gw_types::offchain::{CellInfo, CollectedCustodianCells, InputCellInfo, RollupContext};
use gw_types::packed::{
    AllowedTypeHash, CellDep, CellInput, CellOutput, CustodianLockArgs, DepositRequest,
    GlobalState, LastFinalizedWithdrawal, OutPoint, RawWithdrawalRequest, RollupAction,
    RollupActionUnion, RollupConfig, RollupSubmitBlock, Script, ScriptVec, StakeLockArgs,
    WithdrawalRequest, WithdrawalRequestExtra, WitnessArgs,
};
use gw_types::prelude::{Pack, PackVec, Unpack};
use gw_utils::transaction_skeleton::TransactionSkeleton;

const CKB: u64 = 100000000;
const MAX_MEM_BLOCK_WITHDRAWALS: u8 = 50;

#[tokio::test]
async fn test_finalize_withdrawal_to_owner() {
    let _ = env_logger::builder().is_test(true).try_init();

    const CONTRACT_CELL_CAPACITY: u64 = 1000 * CKB;
    let always_type = random_always_success_script(None);
    let always_cell = CellInfo {
        out_point: OutPoint::new_builder()
            .tx_hash(rand::random::<[u8; 32]>().pack())
            .build(),
        output: CellOutput::new_builder()
            .capacity(CONTRACT_CELL_CAPACITY.pack())
            .type_(Some(always_type.clone()).pack())
            .build(),
        data: ALWAYS_SUCCESS_PROGRAM.clone(),
    };

    let sudt_script = Script::new_builder()
        .code_hash(always_type.hash().pack())
        .hash_type(ScriptHashType::Type.into())
        .args(vec![rand::random::<u8>(), 32].pack())
        .build();
    let sudt_type_cell = always_cell.clone();
    let sudt_scripts_map =
        HashMap::from([(H256::from(sudt_script.hash()), sudt_script.clone()); 1]);

    let stake_lock_type = random_always_success_script(None);
    let stake_lock_cell = CellInfo {
        out_point: OutPoint::new_builder()
            .tx_hash(rand::random::<[u8; 32]>().pack())
            .build(),
        output: CellOutput::new_builder()
            .capacity(CONTRACT_CELL_CAPACITY.pack())
            .type_(Some(stake_lock_type.clone()).pack())
            .build(),
        data: STAKE_LOCK_PROGRAM.clone(),
    };

    let deposit_lock_type = random_always_success_script(None);

    let custodian_lock_type = random_always_success_script(None);
    let custodian_lock_cell = CellInfo {
        out_point: OutPoint::new_builder()
            .tx_hash(rand::random::<[u8; 32]>().pack())
            .build(),
        output: CellOutput::new_builder()
            .capacity(CONTRACT_CELL_CAPACITY.pack())
            .type_(Some(custodian_lock_type.clone()).pack())
            .build(),
        data: CUSTODIAN_LOCK_PROGRAM.clone(),
    };

    let rollup_config = RollupConfig::new_builder()
        .stake_script_type_hash(stake_lock_type.hash().pack())
        .custodian_script_type_hash(custodian_lock_type.hash().pack())
        .deposit_script_type_hash(deposit_lock_type.hash().pack())
        .l1_sudt_script_type_hash(always_type.hash().pack())
        .allowed_eoa_type_hashes(
            vec![AllowedTypeHash::new(
                AllowedEoaType::Eth,
                *ALWAYS_SUCCESS_CODE_HASH,
            )]
            .pack(),
        )
        .finality_blocks(1u64.pack())
        .build();
    let rollup_config_type = random_always_success_script(None);
    let rollup_config_cell = CellInfo {
        out_point: OutPoint::new_builder()
            .tx_hash(rand::random::<[u8; 32]>().pack())
            .build(),
        output: CellOutput::new_builder()
            .capacity(CONTRACT_CELL_CAPACITY.pack())
            .type_(Some(rollup_config_type).pack())
            .build(),
        data: rollup_config.as_bytes(),
    };

    let last_finalized_block_number = 100u64;
    let global_state = GlobalState::new_builder()
        .last_finalized_block_number(last_finalized_block_number.pack())
        .rollup_config_hash(rollup_config.hash().pack())
        .build();

    let state_validator_type = random_always_success_script(None);
    let state_validator_cell = CellInfo {
        out_point: OutPoint::new_builder()
            .tx_hash(rand::random::<[u8; 32]>().pack())
            .build(),
        output: CellOutput::new_builder()
            .capacity(CONTRACT_CELL_CAPACITY.pack())
            .type_(Some(state_validator_type.clone()).pack())
            .build(),
        data: STATE_VALIDATOR_TYPE_PROGRAM.clone(),
    };

    let rollup_type_script = Script::new_builder()
        .code_hash(state_validator_type.hash().pack())
        .hash_type(ScriptHashType::Type.into())
        .args(vec![1u8; 32].pack())
        .build();
    let rollup_script_hash: H256 = rollup_type_script.hash().into();
    let rollup_cell = CellInfo {
        data: global_state.as_bytes(),
        out_point: OutPoint::new_builder()
            .tx_hash(rand::random::<[u8; 32]>().pack())
            .build(),
        output: CellOutput::new_builder()
            .type_(Some(rollup_type_script.clone()).pack())
            .build(),
    };

    let mut chain =
        TestChain::setup_with_config(rollup_type_script.clone(), rollup_config.clone()).await;
    assert_ne!(chain.last_global_state().version_u8(), 2);

    let rollup_context = RollupContext {
        rollup_script_hash,
        rollup_config: rollup_config.clone(),
    };

    let contracts_dep = ContractsCellDep {
        rollup_cell_type: CellDep::new_builder()
            .out_point(state_validator_cell.out_point.clone())
            .build()
            .into(),
        l1_sudt_type: CellDep::new_builder()
            .out_point(sudt_type_cell.out_point.clone())
            .build()
            .into(),
        custodian_cell_lock: CellDep::new_builder()
            .out_point(custodian_lock_cell.out_point.clone())
            .build()
            .into(),
        ..Default::default()
    };

    // Deposit random accounts and upgrade global state version to v2
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

    let deposits: Vec<_> = { accounts.iter() }
        .map(|account_script| {
            DepositRequest::new_builder()
                .capacity(DEPOSIT_CAPACITY.pack())
                .sudt_script_hash(sudt_script.hash().pack())
                .amount(DEPOSIT_AMOUNT.pack())
                .script(account_script.to_owned())
                .registry_id(gw_common::builtins::ETH_REGISTRY_ACCOUNT_ID.pack())
                .build()
        })
        .collect();

    chain.produce_block(deposits, vec![]).await.unwrap();
    assert_eq!(chain.last_global_state().version_u8(), 2);

    let input_rollup_cell = CellInfo {
        data: chain.last_global_state().as_bytes(),
        out_point: OutPoint::new_builder()
            .tx_hash(rand::random::<[u8; 32]>().pack())
            .build(),
        output: CellOutput::new_builder()
            .type_(Some(rollup_type_script.clone()).pack())
            .lock(random_always_success_script(None))
            .build(),
    };

    // Generate random withdrawals
    const WITHDRAWAL_CAPACITY: u64 = 1000 * CKB;
    const WITHDRAWAL_AMOUNT: u128 = 100;
    let (withdrawals, withdrawals_map): (Vec<_>, HashMap<_, _>) = {
        let extras = accounts.iter().map(|account_script| {
            let raw = RawWithdrawalRequest::new_builder()
                .capacity(WITHDRAWAL_CAPACITY.pack())
                .amount(WITHDRAWAL_AMOUNT.pack())
                .account_script_hash(account_script.hash().pack())
                .owner_lock_hash(account_script.hash().pack())
                .sudt_script_hash(sudt_script.hash().pack())
                .registry_id(gw_common::builtins::ETH_REGISTRY_ACCOUNT_ID.pack())
                .build();
            let req = WithdrawalRequest::new_builder().raw(raw).build();
            WithdrawalRequestExtra::new_builder()
                .request(req)
                .owner_lock(account_script.to_owned())
                .build()
        });

        extras
            .map(|w| (w.clone(), (H256::from(w.hash()), w)))
            .unzip()
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
        let mut mem_pool = chain.mem_pool().await;
        let provider = DummyMemPoolProvider {
            deposit_cells: vec![],
            fake_blocktime: Duration::from_millis(0),
        };
        mem_pool.set_provider(Box::new(provider));

        for withdrawal in withdrawals {
            mem_pool.push_withdrawal_request(withdrawal).await.unwrap();
        }

        mem_pool.reset_mem_block().await.unwrap();
        assert_eq!(mem_pool.mem_block().withdrawals().len(), accounts.len());
    }

    const BLOCK_TIMESTAMP: u64 = 10000u64;
    let withdrawal_block_result = {
        let mut mem_pool = chain.mem_pool().await;
        construct_block_with_timestamp(&chain.inner, &mut mem_pool, vec![], BLOCK_TIMESTAMP, true)
            .await
            .unwrap()
    };
    assert_eq!(
        withdrawal_block_result.block.withdrawals().len(),
        accounts.len()
    );
    let withdrawal_block_number = withdrawal_block_result.block.raw().number().unpack();

    // Check submit without withdrawal output cells should pass state validator contract
    const STAKE_CAPACITY: u64 = 1000 * CKB;
    let input_stake_cell = {
        let mut lock_args = rollup_script_hash.as_slice().to_vec();
        lock_args.extend_from_slice(StakeLockArgs::default().as_slice());

        let stake_lock = Script::new_builder()
            .code_hash(stake_lock_type.hash().pack())
            .hash_type(ScriptHashType::Type.into())
            .args(lock_args.pack())
            .build();

        CellInfo {
            out_point: OutPoint::new_builder()
                .tx_hash(rand::random::<[u8; 32]>().pack())
                .build(),
            output: CellOutput::new_builder()
                .capacity(STAKE_CAPACITY.pack())
                .lock(stake_lock)
                .build(),
            data: Bytes::default(),
        }
    };
    let output_stake = {
        let block_number = withdrawal_block_result.block.raw().number();
        let stake_lock_args = StakeLockArgs::new_builder()
            .stake_block_number(block_number)
            .build();

        let mut lock_args = rollup_script_hash.as_slice().to_vec();
        lock_args.extend_from_slice(stake_lock_args.as_slice());

        let stake_lock = Script::new_builder()
            .code_hash(stake_lock_type.hash().pack())
            .hash_type(ScriptHashType::Type.into())
            .args(lock_args.pack())
            .build();

        let output = CellOutput::new_builder()
            .capacity(STAKE_CAPACITY.pack())
            .lock(stake_lock)
            .build();
        (output, Bytes::default())
    };

    let output_rollup_cell = (
        rollup_cell.output.clone(),
        withdrawal_block_result.global_state.as_bytes(),
    );
    let witness = {
        let rollup_action = RollupAction::new_builder()
            .set(RollupActionUnion::RollupSubmitBlock(
                RollupSubmitBlock::new_builder()
                    .block(withdrawal_block_result.block.clone())
                    .build(),
            ))
            .build();
        WitnessArgs::new_builder()
            .output_type(Some(rollup_action.as_bytes()).pack())
            .build()
    };
    let account_scripts_witness = {
        let scripts = ScriptVec::new_builder().set(accounts.clone()).build();
        WitnessArgs::new_builder()
            .output_type(Some(scripts.as_bytes()).pack())
            .build()
    };

    let input_cell_deps = vec![
        into_input_cell(always_cell.clone()),
        into_input_cell(stake_lock_cell.clone()),
        into_input_cell(state_validator_cell.clone()),
        into_input_cell(rollup_config_cell.clone()),
    ];
    let cell_deps = {
        let deps = input_cell_deps.iter();
        deps.map(|i| {
            CellDep::new_builder()
                .out_point(i.cell.out_point.to_owned())
                .dep_type(DepType::Code.into())
                .build()
        })
        .collect::<Vec<_>>()
    };

    const SINCE_BLOCK_TIMESTAMP_FLAG: u64 = 0x4000_0000_0000_0000;
    let block_since = {
        let input_timestamp = Duration::from_millis(BLOCK_TIMESTAMP).as_secs() + 1;
        SINCE_BLOCK_TIMESTAMP_FLAG | input_timestamp
    };
    let inputs = vec![
        into_input_cell_since(input_rollup_cell, block_since),
        into_input_cell(input_stake_cell.clone()),
    ];
    let outputs = vec![output_rollup_cell.clone(), output_stake];

    let mut tx_skeleton = TransactionSkeleton::default();
    tx_skeleton.cell_deps_mut().extend(cell_deps);
    tx_skeleton.inputs_mut().extend(inputs.clone());
    tx_skeleton
        .witnesses_mut()
        .extend([witness, account_scripts_witness]);
    tx_skeleton.outputs_mut().extend(outputs);
    let tx = tx_skeleton.seal(&[], vec![]).unwrap().transaction;

    let tx_with_context = TxWithContext {
        tx,
        cell_deps: input_cell_deps,
        inputs,
    };
    verify_tx(tx_with_context, 7000_0000u64).expect("pass");

    // Check finalize withdrawal to owner tx
    let withdrawals = { withdrawal_block_result.block.withdrawals() }
        .into_iter()
        .map(|w| withdrawals_map.get(&w.hash().into()).unwrap().to_owned())
        .collect();
    chain
        .apply_block_result(vec![], withdrawals, withdrawal_block_result)
        .await
        .unwrap();

    // Produce block to finalize withdrawals
    let finality_blocks = rollup_config.finality_blocks().unpack();
    for _ in 0..(finality_blocks + 1) {
        chain.produce_block(vec![], vec![]).await.unwrap();
    }

    let rollup_config_cell_dep = CellDep::new_builder()
        .out_point(rollup_config_cell.out_point.to_owned())
        .dep_type(DepType::Code.into())
        .build();

    let input_rollup_cell = CellInfo {
        data: chain.last_global_state().as_bytes(),
        out_point: OutPoint::new_builder()
            .tx_hash(rand::random::<[u8; 32]>().pack())
            .build(),
        output: CellOutput::new_builder()
            .type_(Some(rollup_type_script.clone()).pack())
            .lock(random_always_success_script(None))
            .build(),
    };

    let input_custodian_cell = {
        let mut lock_args = rollup_script_hash.as_slice().to_vec();
        lock_args.extend_from_slice(CustodianLockArgs::default().as_slice());

        let custodian_lock = Script::new_builder()
            .code_hash(custodian_lock_type.hash().pack())
            .hash_type(ScriptHashType::Type.into())
            .args(lock_args.pack())
            .build();

        let mut finalized_sudt = finalized_custodians.sudt.values().collect::<Vec<_>>();
        CellInfo {
            out_point: OutPoint::new_builder()
                .tx_hash(rand::random::<[u8; 32]>().pack())
                .build(),
            output: CellOutput::new_builder()
                .capacity((finalized_custodians.capacity as u64).pack())
                .type_(Some(sudt_script.clone()).pack())
                .lock(custodian_lock)
                .build(),
            data: finalized_sudt.pop().unwrap().0.pack().as_bytes(),
        }
    };

    let finalized_custodians = CollectedCustodianCells {
        cells_info: vec![input_custodian_cell.clone()],
        ..finalized_custodians
    };

    let cell_deps = vec![
        into_input_cell(always_cell.clone()),
        into_input_cell(sudt_type_cell.clone()),
        into_input_cell(rollup_config_cell.clone()),
        into_input_cell(state_validator_cell.clone()),
        into_input_cell(custodian_lock_cell.clone()),
    ];

    let mut finalizer = DummyFinalizer {
        rollup_cell: input_rollup_cell.clone(),
        rollup_context: rollup_context.clone(),
        contracts_dep: Arc::new(contracts_dep.clone()),
        rollup_config_cell_dep,
        store: chain.store().to_owned(),
        withdrawals_map,
        sudt_scripts_map,
        finalized_custodians,
        pending: vec![],
    };

    let inputs = vec![
        into_input_cell(input_rollup_cell),
        into_input_cell(input_custodian_cell),
    ];

    let (last_finalized_withdrawal_block, _) = chain
        .last_global_state()
        .last_finalized_withdrawal()
        .unpack_block_index();

    // Finalize block without withdrawal
    finalizer.pending = (last_finalized_withdrawal_block + 1..=(withdrawal_block_number + 1))
        .map(|bn| BlockWithdrawals::new(chain.store().get_block_by_number(bn).unwrap().unwrap()))
        .collect::<Vec<_>>();

    let (tx, updated_last_finalized) = { finalizer.query_and_finalize_to_owner().await }
        .unwrap()
        .unwrap();

    let expected_updated_last_finalized = LastFinalizedWithdrawal::new_builder()
        .block_number((withdrawal_block_number + 1).pack())
        .withdrawal_index(LastFinalizedWithdrawal::INDEX_ALL_WITHDRAWALS.pack())
        .build();

    assert_eq!(
        updated_last_finalized.as_slice(),
        expected_updated_last_finalized.as_slice()
    );

    let tx_with_context = TxWithContext {
        tx,
        cell_deps: cell_deps.clone(),
        inputs: inputs.clone(),
    };

    verify_tx(tx_with_context, 7000_0000u64).expect("pass");

    // Finalize all withdrawals
    finalizer.pending = (last_finalized_withdrawal_block + 1..=withdrawal_block_number)
        .map(|bn| BlockWithdrawals::new(chain.store().get_block_by_number(bn).unwrap().unwrap()))
        .collect::<Vec<_>>();

    let (tx, updated_last_finalized) = { finalizer.query_and_finalize_to_owner().await }
        .unwrap()
        .unwrap();

    let expected_updated_last_finalized = LastFinalizedWithdrawal::new_builder()
        .block_number(withdrawal_block_number.pack())
        .withdrawal_index(LastFinalizedWithdrawal::INDEX_ALL_WITHDRAWALS.pack())
        .build();

    assert_eq!(
        updated_last_finalized.as_slice(),
        expected_updated_last_finalized.as_slice()
    );

    let tx_with_context = TxWithContext {
        tx,
        cell_deps: cell_deps.clone(),
        inputs: inputs.clone(),
    };

    verify_tx(tx_with_context, 7000_0000u64).expect("pass");

    // Finalize partial withdrawals
    finalizer.pending = (last_finalized_withdrawal_block + 1..=withdrawal_block_number)
        .map(|bn| BlockWithdrawals::new(chain.store().get_block_by_number(bn).unwrap().unwrap()))
        .collect::<Vec<_>>();
    let last_block_withdrawals = finalizer.pending.pop().unwrap();
    { &mut finalizer.pending }.push(last_block_withdrawals.take(1).unwrap());

    let (tx, updated_last_finalized) = { finalizer.query_and_finalize_to_owner().await }
        .unwrap()
        .unwrap();

    let expected_updated_last_finalized = LastFinalizedWithdrawal::new_builder()
        .block_number(withdrawal_block_number.pack())
        .withdrawal_index(0u32.pack())
        .build();

    assert_eq!(
        updated_last_finalized.as_slice(),
        expected_updated_last_finalized.as_slice()
    );

    let tx_with_context = TxWithContext {
        tx,
        cell_deps: cell_deps.clone(),
        inputs: inputs.clone(),
    };

    verify_tx(tx_with_context, 7000_0000u64).expect("pass");

    // Finalize rest of withrawals
    let last_finalized = updated_last_finalized;
    let global_state = { chain.last_global_state().clone() }
        .as_builder()
        .last_finalized_withdrawal(last_finalized.clone())
        .build();
    finalizer.rollup_cell.data = global_state.as_bytes();

    let mut inputs = inputs;
    inputs[0] = into_input_cell(finalizer.rollup_cell.clone());

    finalizer.pending = vec![BlockWithdrawals::from_rest(
        { chain.store() }
            .get_block_by_number(last_finalized.block_number().unpack())
            .unwrap()
            .unwrap(),
        &last_finalized,
    )
    .unwrap()
    .unwrap()];

    let (tx, updated_last_finalized) = { finalizer.query_and_finalize_to_owner().await }
        .unwrap()
        .unwrap();

    let expected_updated_last_finalized = LastFinalizedWithdrawal::new_builder()
        .block_number(withdrawal_block_number.pack())
        .withdrawal_index(LastFinalizedWithdrawal::INDEX_ALL_WITHDRAWALS.pack())
        .build();

    assert_eq!(
        updated_last_finalized.as_slice(),
        expected_updated_last_finalized.as_slice()
    );

    let tx_with_context = TxWithContext {
        tx,
        cell_deps: cell_deps.clone(),
        inputs: inputs.clone(),
    };

    verify_tx(tx_with_context, 7000_0000u64).expect("pass");
}

struct DummyFinalizer {
    rollup_cell: CellInfo,
    rollup_context: RollupContext,
    contracts_dep: Arc<ContractsCellDep>,
    rollup_config_cell_dep: CellDep,
    store: Store,
    withdrawals_map: HashMap<H256, WithdrawalRequestExtra>,
    sudt_scripts_map: HashMap<H256, Script>,
    finalized_custodians: CollectedCustodianCells,
    pending: Vec<BlockWithdrawals>,
}

#[async_trait]
impl FinalizeWithdrawalToOwner for DummyFinalizer {
    fn rollup_context(&self) -> &RollupContext {
        &self.rollup_context
    }

    fn contracts_dep(&self) -> Guard<Arc<ContractsCellDep>> {
        Guard::from_inner(Arc::clone(&self.contracts_dep))
    }

    fn rollup_deps(&self) -> Vec<CellDep> {
        vec![
            self.contracts_dep().rollup_cell_type.clone().into(),
            self.rollup_config_cell_dep.clone(),
        ]
    }

    fn transaction_skeleton(&self) -> TransactionSkeleton {
        TransactionSkeleton::default()
    }

    fn generate_block_proof(
        &self,
        block_withdrawals: &[BlockWithdrawals],
    ) -> Result<CompiledMerkleProof> {
        let tx_db = self.store.begin_transaction();
        let block_smt = tx_db.block_smt()?;
        let blocks = block_withdrawals.iter().map(|bw| bw.block());

        Ok(generate_block_proof(&block_smt, blocks)?)
    }

    fn get_withdrawal_extras(
        &self,
        _block_withdrawals: &[BlockWithdrawals],
    ) -> Result<HashMap<H256, WithdrawalRequestExtra>> {
        Ok(self.withdrawals_map.clone())
    }

    fn get_sudt_scripts(
        &self,
        _block_withdrawals: &[BlockWithdrawals],
    ) -> Result<HashMap<H256, Script>> {
        Ok(self.sudt_scripts_map.clone())
    }

    fn get_pending_finalized_withdrawals(
        &self,
        _last_finalized_withdrawal: &LastFinalizedWithdrawal,
        _last_finalized_block_number: u64,
    ) -> Result<Option<Vec<BlockWithdrawals>>> {
        Ok(Some(self.pending.clone()))
    }

    async fn query_rollup_cell(&self) -> anyhow::Result<Option<InputCellInfo>> {
        let input_cell = InputCellInfo {
            input: CellInput::new_builder()
                .previous_output(self.rollup_cell.out_point.clone())
                .build(),
            cell: self.rollup_cell.clone(),
        };
        Ok(Some(input_cell))
    }

    async fn query_finalized_custodians(
        &self,
        _last_finalized_block_number: u64,
        _withdrawals: &[BlockWithdrawals],
    ) -> Result<CollectedCustodianCells> {
        Ok(self.finalized_custodians.clone())
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

fn into_input_cell_since(cell: CellInfo, since: u64) -> InputCellInfo {
    InputCellInfo {
        input: CellInput::new_builder()
            .previous_output(cell.out_point.clone())
            .since(since.pack())
            .build(),
        cell,
    }
}

fn random_always_success_script(opt_rollup_script_hash: Option<&H256>) -> Script {
    let random_bytes: [u8; 20] = rand::random();
    Script::new_builder()
        .code_hash(ALWAYS_SUCCESS_CODE_HASH.clone().pack())
        .hash_type(ScriptHashType::Data.into())
        .args({
            let mut args = opt_rollup_script_hash
                .map(|h| h.as_slice().to_vec())
                .unwrap_or_else(|| rand::random::<[u8; 32]>().to_vec());
            // .unwrap_or_default();
            args.extend_from_slice(&random_bytes);
            args.pack()
        })
        .build()
}
