use std::net::SocketAddr;
use std::time::Duration;

use crate::testing_tool::chain::{
    build_sync_tx, chain_generator, construct_block, setup_chain, ALWAYS_SUCCESS_CODE_HASH,
    ALWAYS_SUCCESS_PROGRAM, WITHDRAWAL_LOCK_PROGRAM,
};
use crate::testing_tool::mem_pool_provider::DummyMemPoolProvider;
use crate::testing_tool::verify_tx::{verify_tx, TxWithContext};

use async_jsonrpc_client::{HttpClient, Output, Params, Transport};
use async_trait::async_trait;
use ckb_types::prelude::{Builder, Entity};
use gw_block_producer::test_mode_control::TestModeControl;
use gw_block_producer::withdrawal_unlocker::BuildUnlockWithdrawalToOwner;
use gw_chain::chain::{L1Action, L1ActionContext, SyncParam};
use gw_common::H256;
use gw_config::{BlockProducerConfig, NodeMode};
use gw_rpc_client::rpc_client::RPCClient;
use gw_rpc_server::registry::{Registry, RegistryArgs};
use gw_rpc_server::server::start_jsonrpc_server;
use gw_types::core::ScriptHashType;
use gw_types::offchain::{CellInfo, CollectedCustodianCells, InputCellInfo, RollupContext};
use gw_types::packed::{
    CellDep, CellInput, CellOutput, DepositRequest, GlobalState, L2BlockCommittedInfo, OutPoint,
    RawWithdrawalRequest, RollupConfig, Script, WithdrawalLockArgs, WithdrawalRequest,
    WithdrawalRequestExtra,
};
use gw_types::prelude::Pack;
use gw_utils::transaction_skeleton::TransactionSkeleton;
use serde_json::json;

const CKB: u64 = 100000000;

#[test]
#[ignore]
fn test_push_withdrawal_with_owner_lock() {
    let _ = env_logger::builder().is_test(true).try_init();

    let rollup_type_script = Script::default();
    let rollup_script_hash: H256 = rollup_type_script.hash().into();
    let rollup_cell = CellOutput::new_builder()
        .type_(Some(rollup_type_script.clone()).pack())
        .build();
    let mut chain = setup_chain(rollup_type_script.clone());

    // Deposit 2 accounts
    const DEPOSIT_CAPACITY: u64 = 1000000 * CKB;
    let accounts: Vec<_> = (0..2)
        .map(|_| random_always_success_script(Some(&rollup_script_hash)))
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
        let mut mem_pool = smol::block_on(mem_pool.lock());
        construct_block(&chain, &mut mem_pool, deposits.clone().collect()).unwrap()
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
    chain.sync(param).unwrap();
    assert!(chain.last_sync_event().is_success());

    // Generate random withdrawals, deposits, txs
    const WITHDRAWAL_CAPACITY: u64 = 1000 * CKB;
    let alice = accounts.first().unwrap().to_owned();
    let withdrawal: WithdrawalRequestExtra = {
        let raw = RawWithdrawalRequest::new_builder()
            .capacity(WITHDRAWAL_CAPACITY.pack())
            .account_script_hash(alice.hash().pack())
            .sudt_script_hash(H256::zero().pack())
            .build();
        WithdrawalRequest::new_builder().raw(raw).build().into()
    };
    let bob = accounts.last().unwrap().to_owned();
    let bob_owner_lock = random_always_success_script(Some(&rollup_script_hash));
    let withdrawal_with_owner_lock = {
        let raw = RawWithdrawalRequest::new_builder()
            .capacity(WITHDRAWAL_CAPACITY.pack())
            .account_script_hash(bob.hash().pack())
            .sudt_script_hash(H256::zero().pack())
            .owner_lock_hash(bob_owner_lock.hash().pack())
            .build();
        let req = WithdrawalRequest::new_builder().raw(raw).build();
        WithdrawalRequestExtra::new_builder()
            .request(req)
            .owner_lock(Some(bob_owner_lock).pack())
            .build()
    };

    // Push withdrawals, deposits and txs
    let finalized_custodians = CollectedCustodianCells {
        capacity: ((accounts.len() as u64 + 1) * WITHDRAWAL_CAPACITY) as u128,
        cells_info: vec![Default::default()],
        ..Default::default()
    };

    {
        let mem_pool = chain.mem_pool().as_ref().unwrap();
        let mut mem_pool = smol::block_on(mem_pool.lock());
        let provider = DummyMemPoolProvider {
            deposit_cells: vec![],
            fake_blocktime: Duration::from_millis(0),
            collected_custodians: finalized_custodians,
        };
        mem_pool.set_provider(Box::new(provider));
        mem_pool.reset_mem_block().unwrap();
    }

    let rpc_addr = random_bindable_socket_addr();
    let _rpc_server = {
        let store = chain.store().clone();
        let mem_pool = chain.mem_pool().clone();
        let generator = chain_generator(&chain, rollup_type_script.clone());
        let rollup_config = generator.rollup_context().rollup_config.to_owned();
        let rollup_context = generator.rollup_context().to_owned();
        let rpc_client = {
            let indexer_client =
                HttpClient::new(format!("http://{}", random_bindable_socket_addr())).unwrap();
            let ckb_client =
                HttpClient::new(format!("http://{}", random_bindable_socket_addr())).unwrap();
            let rollup_type_script =
                ckb_types::packed::Script::new_unchecked(rollup_type_script.as_bytes());
            RPCClient::new(
                rollup_type_script,
                rollup_context,
                ckb_client,
                indexer_client,
            )
        };

        let args: RegistryArgs<TestModeControl> = RegistryArgs {
            store,
            mem_pool,
            generator,
            tests_rpc_impl: None,
            rollup_config,
            mem_pool_config: Default::default(),
            node_mode: NodeMode::FullNode,
            rpc_client,
            send_tx_rate_limit: Default::default(),
            server_config: Default::default(),
            fee_config: Default::default(),
        };

        smol::spawn(start_jsonrpc_server(
            rpc_addr.parse().unwrap(),
            Registry::new(args),
        ))
        .detach();
    };

    let godwoken_rpc_client = HttpClient::new(format!("http://{}", rpc_addr)).unwrap();
    // let resp = smol::block_on(godwoken_rpc_client.request("gw_get_tip_block_hash", None)).unwrap();
    // assert!(matches!(resp, Output::Success(_)), "get tip block hash");

    let resp = smol::block_on(godwoken_rpc_client.request(
        "gw_submit_withdrawal_request_2",
        Some(Params::Array(vec![json!(format!(
            "0x{}",
            hex::encode(withdrawal.as_bytes())
        ))])),
    ))
    .unwrap();
    assert!(matches!(resp, Output::Success(_)), "submit withdrawal");

    let resp = smol::block_on(godwoken_rpc_client.request(
        "gw_submit_withdrawal_request_2",
        Some(Params::Array(vec![json!(format!(
            "0x{}",
            hex::encode(withdrawal_with_owner_lock.as_bytes())
        ))])),
    ))
    .unwrap();
    assert!(matches!(resp, Output::Success(_)), "submit withdrawal");

    // Check restore withdrawals, deposits and txs
    {
        let mut count = 10;
        while count > 0 {
            {
                let mem_pool = chain.mem_pool().as_ref().unwrap();
                let mem_pool = smol::block_on(mem_pool.lock());

                if mem_pool.mem_block().withdrawals().len() == 2 {
                    break;
                }
            }
            smol::block_on(smol::Timer::after(Duration::from_secs(1)));
            count -= 1;
        }
    }

    let block_result = {
        let mem_pool = chain.mem_pool().as_ref().unwrap();
        let mut mem_pool = smol::block_on(mem_pool.lock());
        construct_block(&chain, &mut mem_pool, vec![]).unwrap()
    };

    assert_eq!(block_result.block.withdrawals().len(), 2);
    assert_eq!(
        block_result.withdrawal_extras.first().unwrap().as_slice(),
        withdrawal.as_slice()
    );
    assert_eq!(
        block_result.withdrawal_extras.last().unwrap().as_slice(),
        withdrawal_with_owner_lock.as_slice()
    );
}

#[test]
fn test_build_unlock_to_owner_tx() {
    let _ = env_logger::builder().is_test(true).try_init();

    let last_finalized_block_number = 100u64;
    let global_state = GlobalState::new_builder()
        .last_finalized_block_number(last_finalized_block_number.pack())
        .build();

    let rollup_type = random_always_success_script(None);
    let rollup_cell = CellInfo {
        data: global_state.as_bytes(),
        out_point: OutPoint::new_builder()
            .tx_hash(rand::random::<[u8; 32]>().pack())
            .build(),
        output: CellOutput::new_builder()
            .type_(Some(rollup_type.clone()).pack())
            .build(),
    };

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

    let withdrawal_count = rand::random::<u8>() % 20 + 5;
    let random_withdrawals: Vec<_> = (0..withdrawal_count)
        .map(|_| {
            let owner_lock = random_always_success_script(None);

            let lock_args = WithdrawalLockArgs::new_builder()
                .owner_lock_hash(owner_lock.hash().pack())
                .withdrawal_block_number((last_finalized_block_number - 1).pack())
                .build();

            let mut args = rollup_type.hash().to_vec();
            args.extend_from_slice(&lock_args.as_bytes());
            args.extend_from_slice(&(owner_lock.as_bytes().len() as u32).to_be_bytes());
            args.extend_from_slice(&owner_lock.as_bytes());

            let lock = Script::new_builder()
                .code_hash(withdrawal_lock_type.hash().pack())
                .hash_type(ScriptHashType::Type.into())
                .args(args.pack())
                .build();

            CellInfo {
                output: CellOutput::new_builder()
                    .type_(Some(sudt_script.clone()).pack())
                    .lock(lock)
                    .build(),
                data: 100u128.pack().as_bytes(),
                out_point: OutPoint::new_builder()
                    .tx_hash(rand::random::<[u8; 32]>().pack())
                    .build(),
            }
        })
        .collect();

    let unlocker = DummyUnlocker {
        rollup_cell: rollup_cell.clone(),
        rollup_context,
        block_producer_config,
        withdrawals: random_withdrawals.clone(),
    };

    let cell_deps = vec![
        into_input_cell(rollup_cell),
        into_input_cell(always_cell),
        into_input_cell(withdrawal_lock_cell),
    ];
    let inputs = random_withdrawals
        .into_iter()
        .map(into_input_cell)
        .collect();
    let tx = smol::block_on(unlocker.query_and_unlock_to_owner())
        .expect("unlock")
        .expect("some withdrawals tx");

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

fn random_bindable_socket_addr() -> String {
    let mut count = 20;
    let socket = socket2::Socket::new(socket2::Domain::IPV4, socket2::Type::STREAM, None).unwrap();

    while count > 0 {
        let random_port = (rand::random::<u32>() + 1024) % 65534;
        let addr = format!("127.0.0.1:{}", random_port);
        if let Ok(()) = socket.bind(&addr.parse::<SocketAddr>().unwrap().into()) {
            return addr;
        }
        count -= 1;
    }

    panic!("no random bindable address");
}
