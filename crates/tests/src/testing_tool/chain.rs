use gw_block_producer::produce_block::{produce_block, ProduceBlockParam, ProduceBlockResult};
use gw_chain::chain::{Chain, L1Action, L1ActionContext, SyncParam};
use gw_common::{blake2b::new_blake2b, H256};
use gw_config::{BackendConfig, GenesisConfig};
use gw_generator::{
    account_lock_manage::{always_success::AlwaysSuccess, AccountLockManage},
    backend_manage::BackendManage,
    genesis::init_genesis,
    Generator,
};
use gw_mem_pool::{custodian::AvailableCustodians, pool::MemPool};
use gw_store::Store;
use gw_types::{
    bytes::Bytes,
    core::ScriptHashType,
    offchain::{CellInfo, DepositInfo, RollupContext},
    packed::{
        CellOutput, DepositRequest, L2BlockCommittedInfo, RawTransaction, RollupAction,
        RollupActionUnion, RollupConfig, RollupSubmitBlock, Script, Transaction, WitnessArgs,
    },
    prelude::*,
};
use lazy_static::lazy_static;
use smol::lock::Mutex;
use std::{collections::HashSet, time::Duration};
use std::{fs, io::Read, path::PathBuf, sync::Arc};

use super::mem_pool_provider::DummyMemPoolProvider;

const SCRIPT_DIR: &'static str = "../../tests-deps/godwoken-scripts/build/debug";
const ALWAYS_SUCCESS_PATH: &'static str = "always-success";

lazy_static! {
    pub static ref ALWAYS_SUCCESS_PROGRAM: Bytes = {
        let mut buf = Vec::new();
        let mut path = PathBuf::new();
        path.push(&SCRIPT_DIR);
        path.push(&ALWAYS_SUCCESS_PATH);
        let mut f = fs::File::open(&path).expect("load program");
        f.read_to_end(&mut buf).expect("read program");
        Bytes::from(buf.to_vec())
    };
    pub static ref ALWAYS_SUCCESS_CODE_HASH: [u8; 32] = {
        let mut buf = [0u8; 32];
        let mut hasher = new_blake2b();
        hasher.update(&ALWAYS_SUCCESS_PROGRAM);
        hasher.finalize(&mut buf);
        buf
    };
}

// meta contract
pub const META_VALIDATOR_PATH: &str =
    "../../tests-deps/godwoken-scripts/c/build/meta-contract-validator";
pub const META_GENERATOR_PATH: &str =
    "../../tests-deps/godwoken-scripts/c/build/meta-contract-generator";
pub const META_VALIDATOR_SCRIPT_TYPE_HASH: [u8; 32] = [1u8; 32];

// simple UDT
pub const SUDT_VALIDATOR_PATH: &str = "../../tests-deps/godwoken-scripts/c/build/sudt-validator";
pub const SUDT_GENERATOR_PATH: &str = "../../tests-deps/godwoken-scripts/c/build/sudt-generator";

pub const DEFAULT_FINALITY_BLOCKS: u64 = 6;

pub fn build_backend_manage(rollup_config: &RollupConfig) -> BackendManage {
    let sudt_validator_script_type_hash: [u8; 32] =
        rollup_config.l2_sudt_validator_script_type_hash().unpack();
    let configs = vec![
        BackendConfig {
            validator_path: META_VALIDATOR_PATH.into(),
            generator_path: META_GENERATOR_PATH.into(),
            validator_script_type_hash: META_VALIDATOR_SCRIPT_TYPE_HASH.into(),
        },
        BackendConfig {
            validator_path: SUDT_VALIDATOR_PATH.into(),
            generator_path: SUDT_GENERATOR_PATH.into(),
            validator_script_type_hash: sudt_validator_script_type_hash.into(),
        },
    ];
    BackendManage::from_config(configs).expect("default backend")
}

pub fn setup_chain(rollup_type_script: Script) -> Chain {
    let mut account_lock_manage = AccountLockManage::default();
    let rollup_config = RollupConfig::new_builder()
        .allowed_eoa_type_hashes(vec![ALWAYS_SUCCESS_CODE_HASH.clone()].pack())
        .finality_blocks(DEFAULT_FINALITY_BLOCKS.pack())
        .build();
    account_lock_manage.register_lock_algorithm(
        ALWAYS_SUCCESS_CODE_HASH.clone().into(),
        Box::new(AlwaysSuccess),
    );
    setup_chain_with_account_lock_manage(rollup_type_script, rollup_config, account_lock_manage)
}

pub fn setup_chain_with_account_lock_manage(
    rollup_type_script: Script,
    rollup_config: RollupConfig,
    account_lock_manage: AccountLockManage,
) -> Chain {
    let store = Store::open_tmp().unwrap();
    let rollup_script_hash = rollup_type_script.hash();
    let genesis_config = GenesisConfig {
        timestamp: 0,
        meta_contract_validator_type_hash: Default::default(),
        rollup_config: rollup_config.clone().into(),
        rollup_type_hash: rollup_script_hash.into(),
        secp_data_dep: Default::default(),
    };
    let genesis_committed_info = L2BlockCommittedInfo::default();
    let backend_manage = build_backend_manage(&rollup_config);
    let rollup_context = RollupContext {
        rollup_script_hash: rollup_script_hash.into(),
        rollup_config: rollup_config.clone(),
    };
    let generator = Arc::new(Generator::new(
        backend_manage,
        account_lock_manage,
        rollup_context.clone(),
    ));
    init_genesis(
        &store,
        &genesis_config,
        genesis_committed_info,
        Bytes::default(),
    )
    .unwrap();
    let provider = DummyMemPoolProvider::default();
    let mem_pool = MemPool::create(
        store.clone(),
        Arc::clone(&generator),
        Box::new(provider),
        None,
    )
    .unwrap();
    Chain::create(
        &rollup_config,
        &rollup_type_script,
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

pub fn apply_block_result(
    chain: &mut Chain,
    rollup_cell: CellOutput,
    block_result: ProduceBlockResult,
    deposit_requests: Vec<DepositRequest>,
    deposit_asset_scripts: HashSet<Script>,
) {
    let l2block = block_result.block.clone();
    let transaction = build_sync_tx(rollup_cell, block_result);
    let l2block_committed_info = L2BlockCommittedInfo::default();

    let update = L1Action {
        context: L1ActionContext::SubmitBlock {
            l2block,
            deposit_requests,
            deposit_asset_scripts,
        },
        transaction,
        l2block_committed_info,
    };
    let param = SyncParam {
        updates: vec![update],
        reverts: Default::default(),
        known_l1_tip: None,
    };
    chain.sync(param).unwrap();
    assert_eq!(chain.last_sync_event().is_success(), true);
}

pub fn construct_block(
    chain: &Chain,
    mem_pool: &mut MemPool,
    deposit_requests: Vec<DepositRequest>,
) -> anyhow::Result<ProduceBlockResult> {
    let stake_cell_owner_lock_hash = H256::zero();
    let db = chain.store().begin_transaction();
    let generator = chain.generator();
    let rollup_config_hash = chain.rollup_config_hash().clone().into();

    let mut available_custodians = AvailableCustodians::default();
    available_custodians.capacity = std::u128::MAX;
    for withdrawal_hash in mem_pool.mem_block().withdrawals().iter() {
        let req = mem_pool.all_withdrawals().get(withdrawal_hash).unwrap();
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
        .map(|deposit| DepositInfo {
            cell: CellInfo {
                out_point: Default::default(),
                output: CellOutput::new_builder()
                    .lock(
                        Script::new_builder()
                            .code_hash(deposit_lock_type_hash.clone())
                            .hash_type(ScriptHashType::Type.into())
                            .args(rollup_script_hash.as_slice().to_vec().pack())
                            .build(),
                    )
                    .capacity(deposit.capacity())
                    .build(),
                data: Default::default(),
            },
            request: deposit,
        })
        .collect();
    let provider = DummyMemPoolProvider {
        deposit_cells,
        fake_blocktime: Duration::from_millis(0),
        available_custodians,
    };
    mem_pool.set_provider(Box::new(provider));
    // refresh mem block
    mem_pool.reset_mem_block()?;

    let block_param = mem_pool.output_mem_block().unwrap();
    let param = ProduceBlockParam {
        stake_cell_owner_lock_hash,
        rollup_config_hash,
        reverted_block_root: H256::default(),
        block_param,
    };
    produce_block(&db, &generator, param)
}
