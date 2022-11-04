use crate::testing_tool::programs::ALWAYS_SUCCESS_CODE_HASH;
use anyhow::Result;
use gw_block_producer::produce_block::{
    generate_produce_block_param, produce_block, ProduceBlockParam, ProduceBlockResult,
};
use gw_chain::chain::{Chain, L1Action, L1ActionContext, SyncParam};
use gw_common::{builtins::ETH_REGISTRY_ACCOUNT_ID, registry_address::RegistryAddress, H256};
use gw_config::{BackendConfig, BackendType, ChainConfig, GenesisConfig, MemPoolConfig, NodeMode};
use gw_generator::{
    account_lock_manage::{always_success::AlwaysSuccess, AccountLockManage},
    backend_manage::BackendManage,
    genesis::init_genesis,
    Generator,
};
use gw_mem_pool::{
    async_trait,
    custodian::AvailableCustodians,
    pool::{MemPool, MemPoolCreateArgs, OutputParam},
    traits::MemPoolProvider,
};
use gw_store::{traits::chain_store::ChainStore, Store};
use gw_types::{
    core::ScriptHashType,
    offchain::{CellInfo, CollectedCustodianCells, DepositInfo, RollupContext},
    packed::{
        CellOutput, DepositLockArgs, DepositRequest, L2BlockCommittedInfo, RawTransaction,
        RollupAction, RollupActionUnion, RollupConfig, RollupSubmitBlock, Script, Transaction,
        WithdrawalRequest, WitnessArgs,
    },
    prelude::*,
};
use std::{
    collections::HashSet,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::sync::Mutex;

// meta contract
pub const META_VALIDATOR_PATH: &str = "../c/build/meta-contract-validator";
pub const META_GENERATOR_PATH: &str = "../c/build/meta-contract-generator";
pub const META_VALIDATOR_SCRIPT_TYPE_HASH: [u8; 32] = [1u8; 32];
pub const ETH_REGISTRY_VALIDATOR_SCRIPT_TYPE_HASH: [u8; 32] = [2u8; 32];

// simple UDT
pub const SUDT_VALIDATOR_PATH: &str = "../c/build/sudt-validator";
pub const SUDT_GENERATOR_PATH: &str = "../c/build/sudt-generator";

#[derive(Debug, Default)]
pub struct DummyMemPoolProvider {
    pub fake_blocktime: Duration,
    pub deposit_cells: Vec<DepositInfo>,
    pub available_custodians: AvailableCustodians,
}

#[async_trait]
impl MemPoolProvider for DummyMemPoolProvider {
    async fn estimate_next_blocktime(&self) -> Result<Duration> {
        let fake_blocktime = self.fake_blocktime;
        Ok(fake_blocktime)
    }
    async fn collect_deposit_cells(&self) -> Result<Vec<DepositInfo>> {
        let deposit_cells = self.deposit_cells.clone();
        Ok(deposit_cells)
    }
    async fn get_cell(
        &self,
        _out_point: gw_types::packed::OutPoint,
    ) -> Result<Option<gw_types::offchain::CellWithStatus>> {
        Ok(None)
    }
    async fn query_available_custodians(
        &self,
        _withdrawals: Vec<WithdrawalRequest>,
        _last_finalized_block_number: u64,
        _rollup_context: RollupContext,
    ) -> Result<CollectedCustodianCells> {
        let available_custodians = self.available_custodians.clone();
        let collected_custodian_cells = CollectedCustodianCells {
            cells_info: Vec::default(),
            capacity: available_custodians.capacity,
            sudt: available_custodians.sudt,
        };
        Ok(collected_custodian_cells)
    }
}

pub fn build_backend_manage(rollup_config: &RollupConfig) -> BackendManage {
    let sudt_validator_script_type_hash: [u8; 32] =
        rollup_config.l2_sudt_validator_script_type_hash().unpack();
    let configs = vec![
        BackendConfig {
            validator_path: META_VALIDATOR_PATH.into(),
            generator_path: META_GENERATOR_PATH.into(),
            validator_script_type_hash: META_VALIDATOR_SCRIPT_TYPE_HASH.into(),
            backend_type: BackendType::Meta,
        },
        BackendConfig {
            validator_path: SUDT_VALIDATOR_PATH.into(),
            generator_path: SUDT_GENERATOR_PATH.into(),
            validator_script_type_hash: sudt_validator_script_type_hash.into(),
            backend_type: BackendType::Sudt,
        },
    ];
    BackendManage::from_config(configs).expect("default backend")
}

pub async fn setup_chain(rollup_type_script: Script, rollup_config: RollupConfig) -> Chain {
    let mut account_lock_manage = AccountLockManage::default();
    account_lock_manage
        .register_lock_algorithm((*ALWAYS_SUCCESS_CODE_HASH).into(), Box::new(AlwaysSuccess));
    let mut chain = setup_chain_with_account_lock_manage(
        rollup_type_script,
        rollup_config,
        account_lock_manage,
    )
    .await;
    chain.complete_initial_syncing().await.unwrap();
    chain
}

pub async fn setup_chain_with_account_lock_manage(
    rollup_type_script: Script,
    rollup_config: RollupConfig,
    account_lock_manage: AccountLockManage,
) -> Chain {
    let store = Store::open_tmp().unwrap();
    let genesis_l2block_committed_info = L2BlockCommittedInfo::default();
    let backend_manage = build_backend_manage(&rollup_config);
    let rollup_script_hash: ckb_types::H256 = rollup_type_script.hash().into();
    let genesis_config = GenesisConfig {
        timestamp: 0,
        meta_contract_validator_type_hash: META_VALIDATOR_SCRIPT_TYPE_HASH.into(),
        eth_registry_validator_type_hash: ETH_REGISTRY_VALIDATOR_SCRIPT_TYPE_HASH.into(),
        rollup_type_hash: rollup_script_hash.clone().0.into(),
        rollup_config: rollup_config.clone().into(),
        secp_data_dep: Default::default(),
    };
    let rollup_context = RollupContext {
        rollup_script_hash: rollup_script_hash.0.into(),
        rollup_config: rollup_config.clone(),
    };
    let generator = Arc::new(Generator::new(
        backend_manage,
        account_lock_manage,
        rollup_context,
    ));
    init_genesis(
        &store,
        &genesis_config,
        genesis_l2block_committed_info,
        Default::default(),
    )
    .unwrap();
    let provider = Box::new(DummyMemPoolProvider::default());
    let mem_pool_config = MemPoolConfig {
        restore_path: tempfile::TempDir::new().unwrap().path().to_path_buf(),
        ..Default::default()
    };
    let args = MemPoolCreateArgs {
        block_producer: RegistryAddress::new(ETH_REGISTRY_ACCOUNT_ID, Vec::default()),
        store: store.clone(),
        generator: Arc::clone(&generator),
        provider,
        error_tx_handler: None,
        error_tx_receipt_notifier: None,
        config: mem_pool_config,
        node_mode: NodeMode::FullNode,
        dynamic_config_manager: Default::default(),
    };
    let mem_pool = MemPool::create(args).await.unwrap();
    Chain::create(
        &rollup_config,
        &rollup_type_script,
        &ChainConfig::default(),
        store,
        generator,
        Some(Arc::new(Mutex::new(mem_pool))),
    )
    .unwrap()
}

pub fn build_sync_tx(
    rollup_cell: CellOutput,
    produce_block_result: ProduceBlockResult,
) -> Transaction {
    let ProduceBlockResult {
        block,
        global_state,
        withdrawal_extras: _,
    } = produce_block_result;
    let action = RollupAction::new_builder()
        .set(RollupActionUnion::RollupSubmitBlock(
            RollupSubmitBlock::new_builder().block(block).build(),
        ))
        .build();
    let witness = WitnessArgs::new_builder()
        .output_type(Pack::<_>::pack(&Some(action.as_bytes())))
        .build();
    let raw = RawTransaction::new_builder()
        .outputs(vec![rollup_cell].pack())
        .outputs_data(vec![global_state.as_bytes()].pack())
        .build();
    Transaction::new_builder()
        .raw(raw)
        .witnesses(vec![witness.as_bytes()].pack())
        .build()
}

pub async fn apply_block_result(
    chain: &mut Chain,
    rollup_cell: CellOutput,
    block_result: ProduceBlockResult,
    deposit_requests: Vec<DepositRequest>,
    deposit_asset_scripts: HashSet<Script>,
) {
    let l2block = block_result.block.clone();
    let withdrawals = block_result.withdrawal_extras.clone();
    let transaction = build_sync_tx(rollup_cell, block_result);
    let l2block_committed_info = L2BlockCommittedInfo::default();
    let update = L1Action {
        context: L1ActionContext::SubmitBlock {
            l2block,
            deposit_requests,
            deposit_asset_scripts,
            withdrawals,
        },
        transaction,
        l2block_committed_info,
    };
    let param = SyncParam {
        updates: vec![update],
        reverts: Default::default(),
    };
    chain.sync(param).await.unwrap();
    assert!(chain.last_sync_event().is_success());
}

pub async fn construct_block(
    chain: &Chain,
    mem_pool: &mut MemPool,
    deposit_requests: Vec<DepositRequest>,
) -> anyhow::Result<ProduceBlockResult> {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("timestamp")
        .as_millis() as u64;

    construct_block_from_timestamp(chain, mem_pool, deposit_requests, timestamp).await
}

pub async fn construct_block_from_timestamp(
    chain: &Chain,
    mem_pool: &mut MemPool,
    deposit_requests: Vec<DepositRequest>,
    timestamp: u64,
) -> anyhow::Result<ProduceBlockResult> {
    let stake_cell_owner_lock_hash = H256::zero();
    let db = chain.store().begin_transaction();
    let generator = chain.generator();
    let rollup_config_hash = chain.rollup_config_hash().clone().into();

    let mut available_custodians = AvailableCustodians {
        capacity: std::u128::MAX,
        ..AvailableCustodians::default()
    };
    for withdrawal_hash in mem_pool.mem_block().withdrawals().iter() {
        let req = db.get_mem_pool_withdrawal(withdrawal_hash)?.unwrap();
        if 0 == req.raw().amount().unpack() {
            continue;
        }

        let sudt_script_hash: [u8; 32] = req.raw().sudt_script_hash().unpack();
        available_custodians
            .sudt
            .insert(sudt_script_hash, (std::u128::MAX, Script::default()));
    }

    let deposit_lock_type_hash = generator
        .rollup_context()
        .rollup_config
        .deposit_script_type_hash();
    let rollup_script_hash = generator.rollup_context().rollup_script_hash;

    let deposit_cells = deposit_requests
        .into_iter()
        .map(|deposit| {
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

            DepositInfo {
                cell: CellInfo {
                    out_point: Default::default(),
                    output: CellOutput::new_builder()
                        .lock(
                            Script::new_builder()
                                .code_hash(deposit_lock_type_hash.clone())
                                .hash_type(ScriptHashType::Type.into())
                                .args(lock_args.pack())
                                .build(),
                        )
                        .capacity(deposit.capacity())
                        .build(),
                    data: Default::default(),
                },
                request: deposit,
            }
        })
        .collect();
    let provider = DummyMemPoolProvider {
        deposit_cells,
        fake_blocktime: Duration::from_millis(timestamp),
        available_custodians,
    };
    mem_pool.set_provider(Box::new(provider));
    // refresh mem block
    mem_pool.reset_mem_block().await?;

    let (mem_block, post_block_state) = mem_pool.output_mem_block(&OutputParam::default());
    let (_collected_custodian, block_param) =
        generate_produce_block_param(chain.store(), mem_block, post_block_state).unwrap();
    let param = ProduceBlockParam {
        stake_cell_owner_lock_hash,
        rollup_config_hash,
        reverted_block_root: H256::default(),
        block_param,
    };
    produce_block(&db, generator, param)
}
