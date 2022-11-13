use criterion::*;
use gw_common::{
    builtins::ETH_REGISTRY_ACCOUNT_ID, registry_address::RegistryAddress, smt::SMT, state::State,
    H256,
};
use gw_config::{BackendConfig, BackendForkConfig};
use gw_generator::{
    account_lock_manage::AccountLockManage, backend_manage::BackendManage, error::TransactionError,
    traits::StateExt, Generator,
};
use gw_store::{
    smt::smt_store::SMTStateStore,
    snapshot::StoreSnapshot,
    state::{
        overlay::{mem_state::MemStateTree, mem_store::MemStore},
        traits::JournalDB,
        MemStateDB,
    },
    Store,
};
use gw_traits::{ChainView, CodeStore};
use gw_types::{
    bytes::Bytes,
    core::{AllowedEoaType, ScriptHashType},
    offchain::RunResult,
    packed::{AllowedTypeHash, BlockInfo, Fee},
    packed::{RawL2Transaction, RollupConfig, SUDTArgs, SUDTTransfer, Script},
    prelude::*,
    U256,
};
use gw_utils::RollupContext;

const DUMMY_SUDT_VALIDATOR_SCRIPT_TYPE_HASH: [u8; 32] = [3u8; 32];

// meta contract
const META_VALIDATOR_PATH: &str = "../../.tmp/binaries/godwoken-scripts/meta-contract-validator";
const META_GENERATOR_PATH: &str = "../../.tmp/binaries/godwoken-scripts/meta-contract-generator";
const META_VALIDATOR_SCRIPT_TYPE_HASH: [u8; 32] = [1u8; 32];

// simple UDT
const SUDT_VALIDATOR_PATH: &str = "../../.tmp/binaries/godwoken-scripts/sudt-validator";
const SUDT_GENERATOR_PATH: &str = "../../.tmp/binaries/godwoken-scripts/sudt-generator";

fn build_backend_manage(rollup_config: &RollupConfig) -> BackendManage {
    let sudt_validator_script_type_hash: [u8; 32] =
        rollup_config.l2_sudt_validator_script_type_hash().unpack();
    let configs = vec![
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
    ];
    BackendManage::from_config(vec![BackendForkConfig {
        fork_height: 0,
        backends: configs,
    }])
    .expect("default backend")
}

struct DummyChainStore;

impl ChainView for DummyChainStore {
    fn get_block_hash_by_number(&self, _number: u64) -> Result<Option<H256>, gw_db::error::Error> {
        Err("dummy chain store".to_string().into())
    }
}

fn new_state(store: StoreSnapshot) -> MemStateDB {
    let smt = SMT::new(H256::zero(), SMTStateStore::new(MemStore::new(store)));
    let inner = MemStateTree::new(smt, 0);
    MemStateDB::new(inner)
}

fn new_block_info(block_producer: &RegistryAddress, number: u64, timestamp: u64) -> BlockInfo {
    BlockInfo::new_builder()
        .block_producer(Bytes::from(block_producer.to_bytes()).pack())
        .number(number.pack())
        .timestamp(timestamp.pack())
        .build()
}

fn run_contract_get_result<S: State + CodeStore + JournalDB>(
    rollup_config: &RollupConfig,
    tree: &mut S,
    from_id: u32,
    to_id: u32,
    args: Bytes,
    block_info: &BlockInfo,
) -> Result<RunResult, TransactionError> {
    let raw_tx = RawL2Transaction::new_builder()
        .from_id(from_id.pack())
        .to_id(to_id.pack())
        .args(args.pack())
        .build();
    let backend_manage = build_backend_manage(rollup_config);
    let account_lock_manage = AccountLockManage::default();
    let rollup_ctx = RollupContext {
        rollup_config: rollup_config.clone(),
        rollup_script_hash: [42u8; 32].into(),
        fork_config: Default::default(),
    };
    let generator = Generator::new(
        backend_manage,
        Default::default(),
        account_lock_manage,
        rollup_ctx,
        Default::default(),
    );
    let chain_view = DummyChainStore;
    let run_result = generator
        .execute_transaction(&chain_view, tree, block_info, &raw_tx, Some(u64::MAX), None)
        .map_err(|err| err.downcast::<TransactionError>().expect("tx error"))?;
    tree.finalise()?;
    Ok(run_result)
}

fn run_contract<S: State + CodeStore + JournalDB>(
    rollup_config: &RollupConfig,
    tree: &mut S,
    from_id: u32,
    to_id: u32,
    args: Bytes,
    block_info: &BlockInfo,
) -> Result<Vec<u8>, TransactionError> {
    let run_result =
        run_contract_get_result(rollup_config, tree, from_id, to_id, args, block_info)?;
    Ok(run_result.return_data.to_vec())
}

pub fn bench(c: &mut Criterion) {
    let mut group = c.benchmark_group("throughput");
    group.throughput(Throughput::Elements(1u64));
    group.bench_function("sudt", move |b| {
        b.iter_batched(
            || {
                let db = Store::open_tmp().unwrap();
                let mut tree = new_state(db.get_snapshot());

                let always_success_lock_hash = [255u8; 32];
                let rollup_config = RollupConfig::new_builder()
                    .l2_sudt_validator_script_type_hash(
                        DUMMY_SUDT_VALIDATOR_SCRIPT_TYPE_HASH.pack(),
                    )
                    .allowed_eoa_type_hashes(
                        vec![AllowedTypeHash::new_builder()
                            .hash(always_success_lock_hash.pack())
                            .type_(AllowedEoaType::Eth.into())
                            .build()]
                        .pack(),
                    )
                    .build();

                let init_a_balance = U256::from(10000u128);

                // init accounts
                let _meta = tree
                    .create_account_from_script(
                        Script::new_builder()
                            .code_hash(DUMMY_SUDT_VALIDATOR_SCRIPT_TYPE_HASH.clone().pack())
                            .args([0u8; 64].to_vec().pack())
                            .hash_type(ScriptHashType::Type.into())
                            .build(),
                    )
                    .expect("create account");
                let sudt_id = tree
                    .create_account_from_script(
                        Script::new_builder()
                            .code_hash(DUMMY_SUDT_VALIDATOR_SCRIPT_TYPE_HASH.clone().pack())
                            .args([1u8; 64].to_vec().pack())
                            .hash_type(ScriptHashType::Type.into())
                            .build(),
                    )
                    .expect("create account");
                let a_script = {
                    let mut args = vec![42u8; 32];
                    args.extend([0u8; 20]);
                    Script::new_builder()
                        .code_hash(always_success_lock_hash.pack())
                        .args(args.pack())
                        .hash_type(ScriptHashType::Type.into())
                        .build()
                };
                let a_addr = RegistryAddress::new(ETH_REGISTRY_ACCOUNT_ID, vec![0u8; 20]);
                let a_id = tree
                    .create_account_from_script(a_script)
                    .expect("create account");
                let b_script = {
                    let mut args = vec![42u8; 32];
                    args.extend([1u8; 20]);
                    Script::new_builder()
                        .code_hash(always_success_lock_hash.pack())
                        .args(args.pack())
                        .hash_type(ScriptHashType::Type.into())
                        .build()
                };
                let b_addr = RegistryAddress::new(ETH_REGISTRY_ACCOUNT_ID, vec![1u8; 20]);
                let b_id = tree
                    .create_account_from_script(b_script)
                    .expect("create account");
                let a_script_hash = tree.get_script_hash(a_id).expect("get script hash");
                let b_script_hash = tree.get_script_hash(b_id).expect("get script hash");
                tree.mapping_registry_address_to_script_hash(a_addr.clone(), a_script_hash)
                    .unwrap();
                tree.mapping_registry_address_to_script_hash(b_addr, b_script_hash)
                    .unwrap();

                let block_producer_script = {
                    let mut args = vec![42u8; 32];
                    args.extend([3u8; 20]);
                    Script::new_builder()
                        .code_hash(always_success_lock_hash.pack())
                        .args(args.pack())
                        .hash_type(ScriptHashType::Type.into())
                        .build()
                };
                let block_producer = RegistryAddress::new(ETH_REGISTRY_ACCOUNT_ID, vec![3u8; 20]);
                let block_producer_script_hash = block_producer_script.hash().into();
                tree.mapping_registry_address_to_script_hash(
                    block_producer.clone(),
                    block_producer_script_hash,
                )
                .unwrap();
                let block_info = new_block_info(&block_producer, 1, 0);

                // init balance for a
                tree.mint_sudt(sudt_id, &a_addr, init_a_balance)
                    .expect("init balance");
                (
                    tree,
                    rollup_config,
                    sudt_id,
                    a_id,
                    b_script_hash,
                    block_info,
                )
            },
            |(mut tree, rollup_config, sudt_id, a_id, b_script_hash, block_info)| {
                // transfer from A to B
                let value: U256 = 4000u128.into();
                let fee = 42u128;
                let b_addr = tree
                    .get_registry_address_by_script_hash(ETH_REGISTRY_ACCOUNT_ID, &b_script_hash)
                    .expect("get script hash")
                    .unwrap();
                let args = SUDTArgs::new_builder()
                    .set(
                        SUDTTransfer::new_builder()
                            .to_address(Bytes::from(b_addr.to_bytes()).pack())
                            .amount(value.pack())
                            .fee(
                                Fee::new_builder()
                                    .amount(fee.pack())
                                    .registry_id(ETH_REGISTRY_ACCOUNT_ID.pack())
                                    .build(),
                            )
                            .build(),
                    )
                    .build();
                run_contract(
                    &rollup_config,
                    &mut tree,
                    a_id,
                    sudt_id,
                    args.as_bytes(),
                    &block_info,
                )
                .expect("execute");
            },
            BatchSize::SmallInput,
        );
    });
}

criterion_group! {
    name = sudt;
    config = Criterion::default().sample_size(10);
    targets = bench
}
