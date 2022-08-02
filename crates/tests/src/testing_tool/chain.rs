#![allow(clippy::mutable_key_type)]

use gw_block_producer::produce_block::{
    generate_produce_block_param, produce_block, ProduceBlockParam, ProduceBlockResult,
};
use gw_chain::chain::{Chain, L1Action, L1ActionContext, SyncParam};
use gw_common::{blake2b::new_blake2b, H256};
use gw_config::{BackendConfig, BackendSwitchConfig, ChainConfig, GenesisConfig, MemPoolConfig};
use gw_generator::{
    account_lock_manage::{
        always_success::AlwaysSuccess, secp256k1::Secp256k1Eth, AccountLockManage,
    },
    backend_manage::BackendManage,
    genesis::init_genesis,
    Generator,
};
use gw_mem_pool::pool::{MemPool, MemPoolCreateArgs, OutputParam};
use gw_store::{mem_pool_state::MemPoolState, traits::chain_store::ChainStore, Store};
use gw_types::{
    bytes::Bytes,
    core::{AllowedContractType, AllowedEoaType, ScriptHashType},
    offchain::{CellInfo, DepositInfo, RollupContext},
    packed::{
        AllowedTypeHash, CellOutput, DepositInfoVec, DepositLockArgs, DepositRequest, L2Block,
        OutPoint, RawTransaction, RollupAction, RollupActionUnion, RollupConfig, RollupSubmitBlock,
        Script, Transaction, WithdrawalRequestExtra, WitnessArgs,
    },
    prelude::*,
};
use lazy_static::lazy_static;
use tokio::sync::{Mutex, MutexGuard};

use std::{collections::HashSet, time::Duration};
use std::{fs, path::PathBuf, sync::Arc};

use super::mem_pool_provider::DummyMemPoolProvider;

const SCRIPT_DIR: &str = "../../.tmp/binaries/godwoken-scripts";
const ALWAYS_SUCCESS_PATH: &str = "always-success";
const WITHDRAWAL_LOCK_PATH: &str = "withdrawal-lock";
const STATE_VALIDATOR_TYPE_PATH: &str = "state-validator";
const STAKE_LOCK_PATH: &str = "stake-lock";
const CUSTODIAN_LOCK_PATH: &str = "custodian-lock";
const ETH_ACCOUNT_LOCK_PATH: &str = "eth-account-lock";

lazy_static! {
    pub static ref ALWAYS_SUCCESS_PROGRAM: Bytes = {
        let mut path = PathBuf::new();
        path.push(&SCRIPT_DIR);
        path.push(&ALWAYS_SUCCESS_PATH);
        fs::read(&path).expect("read program").into()
    };
    pub static ref ALWAYS_SUCCESS_CODE_HASH: [u8; 32] = {
        let mut buf = [0u8; 32];
        let mut hasher = new_blake2b();
        hasher.update(&ALWAYS_SUCCESS_PROGRAM);
        hasher.finalize(&mut buf);
        buf
    };
    pub static ref WITHDRAWAL_LOCK_PROGRAM: Bytes = {
        let mut path = PathBuf::new();
        path.push(&SCRIPT_DIR);
        path.push(&WITHDRAWAL_LOCK_PATH);
        fs::read(&path)
            .expect("read withdrawal lock program")
            .into()
    };
    pub static ref WITHDRAWAL_LOCK_CODE_HASH: [u8; 32] = {
        let mut buf = [0u8; 32];
        let mut hasher = new_blake2b();
        hasher.update(&WITHDRAWAL_LOCK_PROGRAM);
        hasher.finalize(&mut buf);
        buf
    };
    pub static ref STATE_VALIDATOR_TYPE_PROGRAM: Bytes = {
        let mut path = PathBuf::new();
        path.push(&SCRIPT_DIR);
        path.push(&STATE_VALIDATOR_TYPE_PATH);
        fs::read(&path)
            .expect("read state validator type program")
            .into()
    };
    pub static ref STATE_VALIDATOR_CODE_HASH: [u8; 32] = {
        let mut buf = [0u8; 32];
        let mut hasher = new_blake2b();
        hasher.update(&STATE_VALIDATOR_TYPE_PROGRAM);
        hasher.finalize(&mut buf);
        buf
    };
    pub static ref STAKE_LOCK_PROGRAM: Bytes = {
        let mut path = PathBuf::new();
        path.push(&SCRIPT_DIR);
        path.push(&STAKE_LOCK_PATH);
        fs::read(&path).expect("read stake lock program").into()
    };
    pub static ref STAKE_LOCK_CODE_HASH: [u8; 32] = {
        let mut buf = [0u8; 32];
        let mut hasher = new_blake2b();
        hasher.update(&STAKE_LOCK_PROGRAM);
        hasher.finalize(&mut buf);
        buf
    };
    pub static ref CUSTODIAN_LOCK_PROGRAM: Bytes = {
        let mut path = PathBuf::new();
        path.push(&SCRIPT_DIR);
        path.push(&CUSTODIAN_LOCK_PATH);
        fs::read(&path).expect("read custodian lock program").into()
    };
    pub static ref CUSTODIAN_LOCK_CODE_HASH: [u8; 32] = {
        let mut buf = [0u8; 32];
        let mut hasher = new_blake2b();
        hasher.update(&CUSTODIAN_LOCK_PROGRAM);
        hasher.finalize(&mut buf);
        buf
    };
    pub static ref ETH_ACCOUNT_LOCK_PROGRAM: Bytes = {
        let mut path = PathBuf::new();
        path.push(&SCRIPT_DIR);
        path.push(&ETH_ACCOUNT_LOCK_PATH);
        fs::read(&path)
            .expect("read eth account lock program")
            .into()
    };
    pub static ref ETH_ACCOUNT_LOCK_CODE_HASH: [u8; 32] = {
        let mut buf = [0u8; 32];
        let mut hasher = new_blake2b();
        hasher.update(&ETH_ACCOUNT_LOCK_PROGRAM);
        hasher.finalize(&mut buf);
        buf
    };
    pub static ref SUDT_VALIDATOR_PROGRAM: Bytes = fs::read(&SUDT_VALIDATOR_PATH)
        .expect("read SUDT program")
        .into();
    pub static ref SUDT_VALIDATOR_CODE_HASH: [u8; 32] = {
        let mut buf = [0u8; 32];
        let mut hasher = new_blake2b();
        hasher.update(&SUDT_VALIDATOR_PROGRAM);
        hasher.finalize(&mut buf);
        buf
    };
    pub static ref ETH_EOA_MAPPING_REGISTRY_VALIDATOR_PROGRAM: Bytes =
        fs::read(&ETH_REGISTRY_VALIDATOR_PATH)
            .expect("read eth eoa mapping registry program")
            .into();
    pub static ref ETH_EOA_MAPPING_REGISTRY_VALIDATOR_CODE_HASH: [u8; 32] = {
        let mut buf = [0u8; 32];
        let mut hasher = new_blake2b();
        hasher.update(&ETH_EOA_MAPPING_REGISTRY_VALIDATOR_PROGRAM);
        hasher.finalize(&mut buf);
        buf
    };
    pub static ref POLYJUICE_VALIDATOR_PROGRAM: Bytes = fs::read(&POLYJUICE_VALIDATOR_PATH)
        .expect("read polyjuice validator program")
        .into();
    pub static ref POLYJUICE_VALIDATOR_CODE_HASH: [u8; 32] = {
        let mut buf = [0u8; 32];
        let mut hasher = new_blake2b();
        hasher.update(&POLYJUICE_VALIDATOR_PROGRAM);
        hasher.finalize(&mut buf);
        buf
    };
}

// meta contract
pub const META_VALIDATOR_PATH: &str =
    "../../.tmp/binaries/godwoken-scripts/meta-contract-validator";
pub const META_GENERATOR_PATH: &str =
    "../../.tmp/binaries/godwoken-scripts/meta-contract-generator";
pub const META_VALIDATOR_SCRIPT_TYPE_HASH: [u8; 32] = [1u8; 32];
pub const ETH_REGISTRY_SCRIPT_TYPE_HASH: [u8; 32] = [2u8; 32];

// simple UDT
pub const SUDT_VALIDATOR_PATH: &str = "../../.tmp/binaries/godwoken-scripts/sudt-validator";
pub const SUDT_GENERATOR_PATH: &str = "../../.tmp/binaries/godwoken-scripts/sudt-generator";

// eth eoa mapping registry
pub const ETH_REGISTRY_VALIDATOR_PATH: &str =
    "../../.tmp/binaries/godwoken-scripts/eth-addr-reg-generator";
pub const ETH_REGISTRY_GENERATOR_PATH: &str =
    "../../.tmp/binaries/godwoken-scripts/eth-addr-reg-validator";

// polyjuice
pub const POLYJUICE_VALIDATOR_PATH: &str = "../../.tmp/binaries/godwoken-polyjuice/validator";
pub const POLYJUICE_GENERATOR_PATH: &str = "../../.tmp/binaries/godwoken-polyjuice/generator";

pub const DEFAULT_FINALITY_BLOCKS: u64 = 6;

pub struct TestChain {
    pub l1_committed_block_number: u64,
    pub rollup_type_script: Script,
    pub inner: Chain,
}

impl TestChain {
    pub async fn setup(rollup_type_script: Script) -> Self {
        let inner = setup_chain(rollup_type_script.clone()).await;

        Self {
            l1_committed_block_number: 1,
            rollup_type_script,
            inner,
        }
    }

    pub async fn update_mem_pool_config(self, mut mem_pool_config: MemPoolConfig) -> Self {
        let Self {
            l1_committed_block_number,
            rollup_type_script,
            inner: chain,
        } = self;

        let rollup_config = chain.generator().rollup_context().rollup_config.to_owned();
        let mut account_lock_manage = AccountLockManage::default();
        account_lock_manage
            .register_lock_algorithm((*ALWAYS_SUCCESS_CODE_HASH).into(), Box::new(AlwaysSuccess));
        account_lock_manage.register_lock_algorithm(
            (*ETH_ACCOUNT_LOCK_CODE_HASH).into(),
            Box::new(Secp256k1Eth::default()),
        );

        let restore_path = {
            let mem_pool = chain.mem_pool().as_ref().unwrap();
            let mem_pool = mem_pool.lock().await;
            mem_pool.restore_manager().path().to_path_buf()
        };
        mem_pool_config.restore_path = restore_path;

        let inner = setup_chain_with_account_lock_manage(
            rollup_type_script.clone(),
            rollup_config,
            account_lock_manage,
            Some(chain.store().to_owned()),
            Some(mem_pool_config),
            None,
        )
        .await;

        Self {
            l1_committed_block_number,
            rollup_type_script,
            inner,
        }
    }

    pub fn chain_id(&self) -> u64 {
        let config = &self.inner.generator().rollup_context().rollup_config;
        config.chain_id().unpack()
    }

    pub async fn mem_pool_state(&self) -> Arc<MemPoolState> {
        let mem_pool = self.inner.mem_pool().as_ref().unwrap().lock().await;
        mem_pool.mem_pool_state()
    }

    pub async fn mem_pool(&self) -> MutexGuard<'_, MemPool> {
        self.inner.mem_pool().as_ref().unwrap().lock().await
    }

    pub fn rollup_type_hash(&self) -> H256 {
        self.inner.generator().rollup_context().rollup_script_hash
    }

    pub fn store(&self) -> &Store {
        self.inner.store()
    }

    pub fn last_valid_block(&self) -> L2Block {
        self.inner
            .store()
            .get_snapshot()
            .get_last_valid_tip_block()
            .unwrap()
    }

    pub async fn produce_block(
        &mut self,
        deposit_info_vec: DepositInfoVec,
        withdrawals: Vec<WithdrawalRequestExtra>,
    ) -> anyhow::Result<()> {
        let rollup_cell = CellOutput::new_builder()
            .type_(Some(self.rollup_type_script.clone()).pack())
            .build();

        let block_result = {
            let mut mem_pool = self.mem_pool().await;
            construct_block(&self.inner, &mut mem_pool, deposit_info_vec.clone()).await?
        };

        self.l1_committed_block_number += 1;
        let update_action = L1Action {
            context: L1ActionContext::SubmitBlock {
                l2block: block_result.block.clone(),
                deposit_info_vec,
                deposit_asset_scripts: Default::default(),
                withdrawals,
            },
            transaction: build_sync_tx(rollup_cell, block_result),
        };
        let param = SyncParam {
            updates: vec![update_action],
            reverts: Default::default(),
        };

        self.inner.sync(param).await?;
        assert!(self.inner.last_sync_event().is_success());

        Ok(())
    }
}

pub fn build_backend_manage(rollup_config: &RollupConfig) -> BackendManage {
    let sudt_validator_script_type_hash: [u8; 32] =
        rollup_config.l2_sudt_validator_script_type_hash().unpack();
    let backends = vec![
        BackendConfig {
            validator_path: META_VALIDATOR_PATH.into(),
            generator_path: META_GENERATOR_PATH.into(),
            validator_script_type_hash: META_VALIDATOR_SCRIPT_TYPE_HASH.into(),
            backend_type: gw_config::BackendType::Meta,
        },
        BackendConfig {
            validator_path: SUDT_VALIDATOR_PATH.into(),
            generator_path: SUDT_GENERATOR_PATH.into(),
            validator_script_type_hash: sudt_validator_script_type_hash.into(),
            backend_type: gw_config::BackendType::Sudt,
        },
        BackendConfig {
            validator_path: ETH_REGISTRY_VALIDATOR_PATH.into(),
            generator_path: ETH_REGISTRY_GENERATOR_PATH.into(),
            validator_script_type_hash: (*ETH_EOA_MAPPING_REGISTRY_VALIDATOR_CODE_HASH).into(),
            backend_type: gw_config::BackendType::EthAddrReg,
        },
        BackendConfig {
            validator_path: POLYJUICE_VALIDATOR_PATH.into(),
            generator_path: POLYJUICE_GENERATOR_PATH.into(),
            validator_script_type_hash: (*POLYJUICE_VALIDATOR_CODE_HASH).into(),
            backend_type: gw_config::BackendType::Polyjuice,
        },
    ];
    BackendManage::from_config(vec![BackendSwitchConfig {
        switch_height: 0,
        backends,
    }])
    .expect("default backend")
}

pub async fn setup_chain(rollup_type_script: Script) -> Chain {
    let mut account_lock_manage = AccountLockManage::default();
    let rollup_config = RollupConfig::new_builder()
        .allowed_eoa_type_hashes(
            vec![
                AllowedTypeHash::new(AllowedEoaType::Eth, *ETH_ACCOUNT_LOCK_CODE_HASH),
                AllowedTypeHash::new(AllowedEoaType::Eth, *ALWAYS_SUCCESS_CODE_HASH),
            ]
            .pack(),
        )
        .allowed_contract_type_hashes(
            vec![
                AllowedTypeHash::new(AllowedContractType::Meta, META_VALIDATOR_SCRIPT_TYPE_HASH),
                AllowedTypeHash::new(AllowedContractType::Sudt, *SUDT_VALIDATOR_CODE_HASH),
                AllowedTypeHash::new(
                    AllowedContractType::EthAddrReg,
                    *ETH_EOA_MAPPING_REGISTRY_VALIDATOR_CODE_HASH,
                ),
                AllowedTypeHash::new(
                    AllowedContractType::Polyjuice,
                    *POLYJUICE_VALIDATOR_CODE_HASH,
                ),
            ]
            .pack(),
        )
        .l2_sudt_validator_script_type_hash(SUDT_VALIDATOR_CODE_HASH.pack())
        .finality_blocks(DEFAULT_FINALITY_BLOCKS.pack())
        .build();
    account_lock_manage
        .register_lock_algorithm((*ALWAYS_SUCCESS_CODE_HASH).into(), Box::new(AlwaysSuccess));
    account_lock_manage.register_lock_algorithm(
        (*ETH_ACCOUNT_LOCK_CODE_HASH).into(),
        Box::new(Secp256k1Eth::default()),
    );
    let chain = setup_chain_with_account_lock_manage(
        rollup_type_script,
        rollup_config,
        account_lock_manage,
        None,
        None,
        None,
    )
    .await;
    chain.notify_new_tip().await.unwrap();
    chain
}

pub async fn setup_chain_with_config(
    rollup_type_script: Script,
    rollup_config: RollupConfig,
) -> Chain {
    let mut account_lock_manage = AccountLockManage::default();
    account_lock_manage
        .register_lock_algorithm((*ALWAYS_SUCCESS_CODE_HASH).into(), Box::new(AlwaysSuccess));
    account_lock_manage.register_lock_algorithm(
        (*ETH_ACCOUNT_LOCK_CODE_HASH).into(),
        Box::new(Secp256k1Eth::default()),
    );
    setup_chain_with_account_lock_manage(
        rollup_type_script,
        rollup_config,
        account_lock_manage,
        None,
        None,
        None,
    )
    .await
}

// Simulate process restart
pub async fn restart_chain(
    chain: &Chain,
    rollup_type_script: Script,
    opt_provider: Option<DummyMemPoolProvider>,
) -> Chain {
    let mut account_lock_manage = AccountLockManage::default();
    let rollup_config = chain.generator().rollup_context().rollup_config.to_owned();
    account_lock_manage
        .register_lock_algorithm((*ALWAYS_SUCCESS_CODE_HASH).into(), Box::new(AlwaysSuccess));
    account_lock_manage.register_lock_algorithm(
        (*ETH_ACCOUNT_LOCK_CODE_HASH).into(),
        Box::new(Secp256k1Eth::default()),
    );
    let restore_path = {
        let mem_pool = chain.mem_pool().as_ref().unwrap();
        let mem_pool = mem_pool.lock().await;
        mem_pool.restore_manager().path().to_path_buf()
    };
    let mem_pool_config = MemPoolConfig {
        restore_path,
        ..Default::default()
    };
    setup_chain_with_account_lock_manage(
        rollup_type_script,
        rollup_config,
        account_lock_manage,
        Some(chain.store().to_owned()),
        Some(mem_pool_config),
        opt_provider,
    )
    .await
}

pub fn chain_generator(chain: &Chain, rollup_type_script: Script) -> Arc<Generator> {
    let rollup_config = chain.generator().rollup_context().rollup_config.to_owned();
    let mut account_lock_manage = AccountLockManage::default();
    account_lock_manage
        .register_lock_algorithm((*ALWAYS_SUCCESS_CODE_HASH).into(), Box::new(AlwaysSuccess));
    account_lock_manage.register_lock_algorithm(
        (*ETH_ACCOUNT_LOCK_CODE_HASH).into(),
        Box::new(Secp256k1Eth::default()),
    );
    let backend_manage = build_backend_manage(&rollup_config);
    let rollup_context = RollupContext {
        rollup_script_hash: rollup_type_script.hash().into(),
        rollup_config,
    };
    Arc::new(Generator::new(
        backend_manage,
        account_lock_manage,
        rollup_context,
        Default::default(),
    ))
}

pub async fn setup_chain_with_account_lock_manage(
    rollup_type_script: Script,
    rollup_config: RollupConfig,
    account_lock_manage: AccountLockManage,
    opt_store: Option<Store>,
    opt_mem_pool_config: Option<MemPoolConfig>,
    opt_mem_pool_provider: Option<DummyMemPoolProvider>,
) -> Chain {
    let store = opt_store.unwrap_or_else(|| Store::open_tmp().unwrap());
    let mem_pool_config = opt_mem_pool_config.unwrap_or_else(|| MemPoolConfig {
        restore_path: tempfile::TempDir::new().unwrap().path().to_path_buf(),
        ..Default::default()
    });
    let rollup_script_hash = rollup_type_script.hash();
    let genesis_config = GenesisConfig {
        timestamp: 0,
        meta_contract_validator_type_hash: META_VALIDATOR_SCRIPT_TYPE_HASH.into(),
        eth_registry_validator_type_hash: ETH_REGISTRY_SCRIPT_TYPE_HASH.into(),
        rollup_config: rollup_config.clone().into(),
        rollup_type_hash: rollup_script_hash.into(),
        secp_data_dep: Default::default(),
    };
    let transaction = Transaction::default();
    init_genesis(
        &store,
        &genesis_config,
        &transaction.as_reader(),
        Bytes::default(),
    )
    .unwrap();
    let backend_manage = build_backend_manage(&rollup_config);
    let rollup_context = RollupContext {
        rollup_script_hash: rollup_script_hash.into(),
        rollup_config: rollup_config.clone(),
    };
    let generator = Arc::new(Generator::new(
        backend_manage,
        account_lock_manage,
        rollup_context,
        Default::default(),
    ));
    let provider = opt_mem_pool_provider.unwrap_or_default();
    let args = MemPoolCreateArgs {
        block_producer: Default::default(),
        store: store.clone(),
        generator: Arc::clone(&generator),
        provider: Box::new(provider),
        config: mem_pool_config,
        node_mode: gw_config::NodeMode::FullNode,
        dynamic_config_manager: Default::default(),
        has_p2p_sync: false,
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
        ..
    } = produce_block_result;
    let rollup_action = {
        let submit_block = RollupSubmitBlock::new_builder().block(block).build();
        RollupAction::new_builder()
            .set(RollupActionUnion::RollupSubmitBlock(submit_block))
            .build()
    };
    let witness = WitnessArgs::new_builder()
        .output_type(Pack::<_>::pack(&Some(rollup_action.as_bytes())))
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
    block_result: ProduceBlockResult,
    deposit_info_vec: DepositInfoVec,
    deposit_asset_scripts: HashSet<Script>,
) {
    let number = block_result.block.raw().number().unpack();
    let hash = block_result.block.hash();
    let store_tx = chain.store().begin_transaction();
    chain
        .update_local(
            &store_tx,
            block_result.block,
            deposit_info_vec,
            deposit_asset_scripts,
            block_result.withdrawal_extras,
            block_result.global_state,
        )
        .await
        .unwrap();
    store_tx
        .set_block_post_finalized_custodian_capacity(
            number,
            &block_result.remaining_capacity.pack().as_reader(),
        )
        .unwrap();
    store_tx.commit().unwrap();
    let mem_pool = chain.mem_pool();
    let mut mem_pool = mem_pool.as_deref().unwrap().lock().await;
    mem_pool
        .notify_new_tip(hash.into(), &Default::default())
        .await
        .unwrap();
}

pub async fn construct_block(
    chain: &Chain,
    mem_pool: &mut MemPool,
    deposit_info_vec: DepositInfoVec,
) -> anyhow::Result<ProduceBlockResult> {
    construct_block_with_timestamp(chain, mem_pool, deposit_info_vec, 0, true).await
}

pub async fn construct_block_with_timestamp(
    chain: &Chain,
    mem_pool: &mut MemPool,
    deposit_info_vec: DepositInfoVec,
    timestamp: u64,
    refresh_mem_pool: bool,
) -> anyhow::Result<ProduceBlockResult> {
    if !refresh_mem_pool {
        assert!(
            deposit_info_vec.is_empty(),
            "skip refresh mem pool, but deposits isn't empty"
        )
    }
    let stake_cell_owner_lock_hash = H256::zero();
    let db = chain.store().begin_transaction();
    let generator = chain.generator();
    let rollup_config_hash = (*chain.rollup_config_hash()).into();

    let provider = DummyMemPoolProvider {
        deposit_cells: deposit_info_vec.unpack(),
        fake_blocktime: Duration::from_millis(timestamp),
    };
    mem_pool.set_provider(Box::new(provider));
    // refresh mem block
    if refresh_mem_pool {
        mem_pool.reset_mem_block(&Default::default()).await?;
    }
    let provider = DummyMemPoolProvider {
        deposit_cells: Vec::default(),
        fake_blocktime: Duration::from_millis(0),
    };
    mem_pool.set_provider(Box::new(provider));

    let (mut mem_block, post_merkle_state) = mem_pool.output_mem_block(&OutputParam::default());
    let remaining_capacity = mem_block.take_finalized_custodians_capacity();
    let block_param = generate_produce_block_param(chain.store(), mem_block, post_merkle_state)?;
    let reverted_block_root = db.get_reverted_block_smt_root().unwrap();
    let param = ProduceBlockParam {
        stake_cell_owner_lock_hash,
        rollup_config_hash,
        reverted_block_root,
        block_param,
    };
    produce_block(&db, generator, param).map(|mut r| {
        r.remaining_capacity = remaining_capacity;
        r
    })
}

pub fn into_deposit_info_cell(
    rollup_context: &RollupContext,
    request: DepositRequest,
) -> DepositInfo {
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
