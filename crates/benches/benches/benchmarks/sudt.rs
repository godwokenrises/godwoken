use criterion::*;
use gw_common::{
    state::{to_short_address, State},
    H256,
};
use gw_config::BackendConfig;
use gw_generator::{
    account_lock_manage::AccountLockManage, backend_manage::BackendManage, dummy_state::DummyState,
    error::TransactionError, traits::StateExt, Generator,
};
use gw_traits::{ChainStore, CodeStore};
use gw_types::{
    bytes::Bytes,
    core::ScriptHashType,
    offchain::{RollupContext, RunResult},
    packed::BlockInfo,
    packed::{RawL2Transaction, RollupConfig, SUDTArgs, SUDTTransfer, Script},
    prelude::*,
};

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
            backend_type: "meta".to_string(),
            validator_path: META_VALIDATOR_PATH.into(),
            generator_path: META_GENERATOR_PATH.into(),
            validator_script_type_hash: META_VALIDATOR_SCRIPT_TYPE_HASH.into(),
        },
        BackendConfig {
            backend_type: "sudt".to_string(),
            validator_path: SUDT_VALIDATOR_PATH.into(),
            generator_path: SUDT_GENERATOR_PATH.into(),
            validator_script_type_hash: sudt_validator_script_type_hash.into(),
        },
    ];
    BackendManage::from_config(configs).expect("default backend")
}

struct DummyChainStore;

impl ChainStore for DummyChainStore {
    fn get_block_hash_by_number(&self, _number: u64) -> Result<Option<H256>, gw_db::error::Error> {
        Err("dummy chain store".to_string().into())
    }
}

fn new_block_info(block_producer_id: u32, number: u64, timestamp: u64) -> BlockInfo {
    BlockInfo::new_builder()
        .block_producer_id(block_producer_id.pack())
        .number(number.pack())
        .timestamp(timestamp.pack())
        .build()
}

fn run_contract_get_result<S: State + CodeStore>(
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
    };
    let generator = Generator::new(
        backend_manage,
        account_lock_manage,
        rollup_ctx,
        Default::default(),
        7000_0000,
    );
    let chain_view = DummyChainStore;
    let run_result = generator.execute_transaction_with_default_max_cycles(
        &chain_view,
        tree,
        block_info,
        &raw_tx,
    )?;
    tree.apply_run_result(&run_result).expect("update state");
    Ok(run_result)
}

fn run_contract<S: State + CodeStore>(
    rollup_config: &RollupConfig,
    tree: &mut S,
    from_id: u32,
    to_id: u32,
    args: Bytes,
    block_info: &BlockInfo,
) -> Result<Vec<u8>, TransactionError> {
    let run_result =
        run_contract_get_result(rollup_config, tree, from_id, to_id, args, block_info)?;
    Ok(run_result.return_data)
}

pub fn bench(c: &mut Criterion) {
    let mut group = c.benchmark_group("throughput");
    group.throughput(Throughput::Elements(1u64));
    group.bench_function("sudt", move |b| {
        b.iter_batched(
            || {
                let mut tree = DummyState::default();

                let rollup_config = RollupConfig::new_builder()
                    .l2_sudt_validator_script_type_hash(
                        DUMMY_SUDT_VALIDATOR_SCRIPT_TYPE_HASH.pack(),
                    )
                    .build();

                let init_a_balance: u128 = 10000;

                // init accounts
                let sudt_id = tree
                    .create_account_from_script(
                        Script::new_builder()
                            .code_hash(DUMMY_SUDT_VALIDATOR_SCRIPT_TYPE_HASH.clone().pack())
                            .args([0u8; 64].to_vec().pack())
                            .hash_type(ScriptHashType::Type.into())
                            .build(),
                    )
                    .expect("create account");
                let a_id = tree
                    .create_account_from_script(
                        Script::new_builder()
                            .code_hash([0u8; 32].pack())
                            .args([0u8; 20].to_vec().pack())
                            .hash_type(ScriptHashType::Type.into())
                            .build(),
                    )
                    .expect("create account");
                let b_id = tree
                    .create_account_from_script(
                        Script::new_builder()
                            .code_hash([0u8; 32].pack())
                            .args([1u8; 20].to_vec().pack())
                            .hash_type(ScriptHashType::Type.into())
                            .build(),
                    )
                    .expect("create account");
                let a_script_hash = tree.get_script_hash(a_id).expect("get script hash");
                let b_script_hash = tree.get_script_hash(b_id).expect("get script hash");
                let block_producer_id = tree
                    .create_account_from_script(
                        Script::new_builder()
                            .code_hash([0u8; 32].pack())
                            .args([3u8; 20].to_vec().pack())
                            .hash_type(ScriptHashType::Type.into())
                            .build(),
                    )
                    .expect("create account");
                let block_info = new_block_info(block_producer_id, 1, 0);

                // init balance for a
                tree.mint_sudt(sudt_id, to_short_address(&a_script_hash), init_a_balance)
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
                let value = 4000u128;
                let fee = 42u128;
                let b_address = to_short_address(&b_script_hash).to_vec();
                let args = SUDTArgs::new_builder()
                    .set(
                        SUDTTransfer::new_builder()
                            .to(b_address.pack())
                            .amount(value.pack())
                            .fee(fee.pack())
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
