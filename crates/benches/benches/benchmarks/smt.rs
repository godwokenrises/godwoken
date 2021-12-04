use criterion::{criterion_group, BenchmarkId, Criterion, Throughput};
use gw_common::{
    blake2b::new_blake2b,
    builtins::CKB_SUDT_ACCOUNT_ID,
    state::{to_short_address, State},
    H256,
};
use gw_config::{BackendConfig, GenesisConfig, StoreConfig};
use gw_db::{schema::COLUMNS, RocksDB};
use gw_generator::{
    account_lock_manage::{always_success::AlwaysSuccess, AccountLockManage},
    backend_manage::BackendManage,
    constants::L2TX_MAX_CYCLES,
    genesis::build_genesis_from_store,
    traits::StateExt,
    Generator,
};
use gw_mem_pool::pool::MemBlockDBMode;
use gw_store::{
    state::state_db::{StateContext, StateTree},
    Store,
};
use gw_traits::{ChainStore, CodeStore};
use gw_types::{
    core::{ScriptHashType, Status},
    offchain::RollupContext,
    packed::{
        AccountMerkleState, BlockInfo, BlockMerkleState, GlobalState, L2Block, RawL2Block,
        RawL2Transaction, RollupConfig, SUDTArgs, SUDTTransfer, Script, SubmitTransactions,
    },
    prelude::*,
};
use pprof::criterion::{Output, PProfProfiler};

// meta contract
const META_VALIDATOR_PATH: &str = "../../.tmp/binaries/godwoken-scripts/meta-contract-validator";
const META_GENERATOR_PATH: &str = "../../.tmp/binaries/godwoken-scripts/meta-contract-generator";
const META_VALIDATOR_SCRIPT_TYPE_HASH: [u8; 32] = [1u8; 32];

// sudt contract
const SUDT_VALIDATOR_PATH: &str = "../../.tmp/binaries/godwoken-scripts/sudt-validator";
const SUDT_GENERATOR_PATH: &str = "../../.tmp/binaries/godwoken-scripts/sudt-generator";
const SUDT_VALIDATOR_SCRIPT_TYPE_HASH: [u8; 32] = [2u8; 32];

// always success lock
const ALWAYS_SUCCESS_LOCK_HASH: [u8; 32] = [3u8; 32];

// rollup type hash
const ROLLUP_TYPE_HASH: [u8; 32] = [4u8; 32];

const CKB_BALANCE: u128 = 100_000_000;

criterion_group! {
    name = smt;
    config = Criterion::default()
    .with_profiler(PProfProfiler::new(100, Output::Flamegraph(None)));
    targets = bench_ckb_transfer
}

pub fn bench_ckb_transfer(c: &mut Criterion) {
    let config = StoreConfig {
        path: "./smt_data/db".parse().unwrap(),
        options_file: Some("./smt_data/db.toml".parse().unwrap()),
        cache_size: Some(1073741824),
        ..Default::default()
    };
    let store = Store::new(RocksDB::open(&config, COLUMNS));
    let ee = BenchExecutionEnvironment::new_with_accounts(store, 7000);

    let mut group = c.benchmark_group("ckb_transfer");
    for txs in (500..=5000).step_by(500) {
        group.sample_size(10);
        group.throughput(Throughput::Elements(txs));
        group.bench_with_input(BenchmarkId::from_parameter(txs), &txs, |b, txs| {
            b.iter(|| {
                ee.accounts_transfer(7000, *txs as usize);
            });
        });
    }
    group.finish();
}

#[allow(dead_code)]
struct Account {
    id: u32,
}

impl Account {
    fn build_script(n: u32) -> Script {
        Script::new_builder()
            .code_hash(ALWAYS_SUCCESS_LOCK_HASH.pack())
            .hash_type(ScriptHashType::Type.into())
            .args(n.to_be_bytes().pack())
            .build()
    }
}

struct BenchChain;
impl ChainStore for BenchChain {
    fn get_block_hash_by_number(&self, _: u64) -> Result<Option<H256>, gw_db::error::Error> {
        unreachable!("bench chain store")
    }
}

struct BenchExecutionEnvironment {
    generator: Generator,
    chain: BenchChain,
    store: Store,
}

impl BenchExecutionEnvironment {
    fn new_with_accounts(store: Store, accounts: u32) -> Self {
        let genesis_config = GenesisConfig {
            meta_contract_validator_type_hash: META_VALIDATOR_SCRIPT_TYPE_HASH.into(),
            rollup_type_hash: ROLLUP_TYPE_HASH.into(),
            rollup_config: RollupConfig::new_builder()
                .l2_sudt_validator_script_type_hash(SUDT_VALIDATOR_SCRIPT_TYPE_HASH.pack())
                .build()
                .into(),
            ..Default::default()
        };

        let rollup_context = RollupContext {
            rollup_config: genesis_config.rollup_config.clone().into(),
            rollup_script_hash: ROLLUP_TYPE_HASH.into(),
        };

        let backend_manage = {
            let configs = vec![
                BackendConfig {
                    validator_path: META_VALIDATOR_PATH.into(),
                    generator_path: META_GENERATOR_PATH.into(),
                    validator_script_type_hash: META_VALIDATOR_SCRIPT_TYPE_HASH.into(),
                },
                BackendConfig {
                    validator_path: SUDT_VALIDATOR_PATH.into(),
                    generator_path: SUDT_GENERATOR_PATH.into(),
                    validator_script_type_hash: SUDT_VALIDATOR_SCRIPT_TYPE_HASH.into(),
                },
            ];
            BackendManage::from_config(configs).expect("bench backend")
        };

        let account_lock_manage = {
            let mut manage = AccountLockManage::default();
            manage
                .register_lock_algorithm(ALWAYS_SUCCESS_LOCK_HASH.into(), Box::new(AlwaysSuccess));
            manage
        };

        let generator = Generator::new(
            backend_manage,
            account_lock_manage,
            rollup_context,
            Default::default(),
        );

        Self::init_genesis(&store, &genesis_config, accounts);

        BenchExecutionEnvironment {
            generator,
            chain: BenchChain,
            store,
        }
    }

    fn accounts_transfer(&self, accounts: u32, count: usize) {
        let db = self.store.begin_transaction();
        let mut state = db.mem_pool_state_tree().unwrap();

        let block_producer_script = Account::build_script(0);
        let block_producer_id = {
            state
                .get_account_id_by_script_hash(&block_producer_script.hash().into())
                .unwrap()
                .unwrap()
        };

        let block_info = BlockInfo::new_builder()
            .block_producer_id(block_producer_id.pack())
            .number(1.pack())
            .timestamp(1.pack())
            .build();

        let block_producer_balance = state
            .get_sudt_balance(
                CKB_SUDT_ACCOUNT_ID,
                to_short_address(&block_producer_script.hash().into()),
            )
            .unwrap();

        let short_addresses = (0..=accounts)
            .map(Account::build_script)
            .map(|s| to_short_address(&s.hash().into()).to_vec())
            .collect::<Vec<Vec<u8>>>();

        let address_offset = block_producer_id; // start from block producer
        let start_account_id = block_producer_id + 1;
        let end_account_id = block_producer_id + accounts;

        // Loop transfer from id to id + 1, until we reach target count
        let mut from_id = start_account_id;
        let mut transfer_count = count;
        while transfer_count > 0 {
            let to_address = {
                let mut to_id = from_id + 1;
                if to_id > end_account_id {
                    to_id = start_account_id;
                }
                short_addresses
                    .get((to_id - address_offset) as usize)
                    .unwrap()
            };

            let args = SUDTArgs::new_builder()
                .set(
                    SUDTTransfer::new_builder()
                        .to(to_address.pack())
                        .amount(1.pack())
                        .fee(1.pack())
                        .build(),
                )
                .build();

            let raw_tx = RawL2Transaction::new_builder()
                .from_id(from_id.pack())
                .to_id(1u32.pack())
                .args(args.as_bytes().pack())
                .build();

            let run_result = self
                .generator
                .execute_transaction(&self.chain, &state, &block_info, &raw_tx, L2TX_MAX_CYCLES)
                .unwrap();

            state.apply_run_result(&run_result).unwrap();

            from_id += 1;
            if from_id > end_account_id {
                from_id = start_account_id;
            }
            transfer_count -= 1;
        }

        db.commit().unwrap();

        let state = db.mem_pool_state_tree().unwrap();
        let post_block_producer_balance = state
            .get_sudt_balance(
                CKB_SUDT_ACCOUNT_ID,
                to_short_address(&block_producer_script.hash().into()),
            )
            .unwrap();

        assert_eq!(
            post_block_producer_balance,
            block_producer_balance + count as u128
        );
    }

    fn generate_accounts(
        state: &mut (impl State + StateExt + CodeStore),
        accounts: u32,
    ) -> Vec<Account> {
        let build_account = |idx: u32| -> Account {
            let account_script = Account::build_script(idx);
            let account_script_hash: H256 = account_script.hash().into();
            let short_address = to_short_address(&account_script_hash);

            let account_id = state.create_account(account_script_hash).unwrap();
            state.insert_script(account_script_hash, account_script);
            state
                .mint_sudt(CKB_SUDT_ACCOUNT_ID, short_address, CKB_BALANCE)
                .unwrap();

            Account { id: account_id }
        };

        (0..accounts).map(build_account).collect()
    }

    fn init_genesis(store: &Store, config: &GenesisConfig, accounts: u32) {
        if store.has_genesis().unwrap() {
            let chain_id = store.get_chain_id().unwrap();
            if chain_id == ROLLUP_TYPE_HASH.into() {
                return;
            } else {
                panic!("store genesis already initialized");
            }
        }

        let db = store.begin_transaction();
        db.setup_chain_id(ROLLUP_TYPE_HASH.into()).unwrap();
        let (db, genesis_state) = build_genesis_from_store(db, config, Default::default()).unwrap();

        let smt = db
            .account_smt_with_merkle_state(genesis_state.genesis.raw().post_account())
            .unwrap();
        let account_count = genesis_state.genesis.raw().post_account().count().unpack();
        let mut state = { StateTree::new(smt, account_count, StateContext::AttachBlock(0)) };

        Self::generate_accounts(&mut state, accounts + 1); // Plus block producer

        let (genesis, global_state) = {
            let prev_state_checkpoint: [u8; 32] =
                state.calculate_state_checkpoint().unwrap().into();
            let submit_txs = SubmitTransactions::new_builder()
                .prev_state_checkpoint(prev_state_checkpoint.pack())
                .build();

            // calculate post state
            let post_account = {
                let root = state.calculate_root().unwrap();
                let count = state.get_account_count().unwrap();
                AccountMerkleState::new_builder()
                    .merkle_root(root.pack())
                    .count(count.pack())
                    .build()
            };

            let raw_genesis = RawL2Block::new_builder()
                .number(0u64.pack())
                .block_producer_id(0u32.pack())
                .parent_block_hash([0u8; 32].pack())
                .timestamp(1.pack())
                .post_account(post_account.clone())
                .submit_transactions(submit_txs)
                .build();

            // generate block proof
            let genesis_hash = raw_genesis.hash();
            let (block_root, block_proof) = {
                let block_key = RawL2Block::compute_smt_key(0);
                let mut smt = db.block_smt().unwrap();
                smt.update(block_key.into(), genesis_hash.into()).unwrap();
                let block_proof = smt
                    .merkle_proof(vec![block_key.into()])
                    .unwrap()
                    .compile(vec![(block_key.into(), genesis_hash.into())])
                    .unwrap();
                let block_root = *smt.root();
                (block_root, block_proof)
            };

            // build genesis
            let genesis = L2Block::new_builder()
                .raw(raw_genesis)
                .block_proof(block_proof.0.pack())
                .build();
            let global_state = {
                let post_block = BlockMerkleState::new_builder()
                    .merkle_root({
                        let root: [u8; 32] = block_root.into();
                        root.pack()
                    })
                    .count(1u64.pack())
                    .build();
                let rollup_config_hash = {
                    let mut hasher = new_blake2b();
                    hasher.update(
                        Into::<RollupConfig>::into(config.rollup_config.clone()).as_slice(),
                    );
                    let mut hash = [0u8; 32];
                    hasher.finalize(&mut hash);
                    hash
                };
                GlobalState::new_builder()
                    .account(post_account)
                    .block(post_block)
                    .status((Status::Running as u8).into())
                    .rollup_config_hash(rollup_config_hash.pack())
                    .tip_block_hash(genesis.hash().pack())
                    .build()
            };

            db.set_block_smt_root(global_state.block().merkle_root().unpack())
                .unwrap();
            (genesis, global_state)
        };

        let prev_txs_state = genesis.as_reader().raw().post_account().to_entity();

        db.set_mem_block_account_smt_root(prev_txs_state.merkle_root().unpack())
            .unwrap();
        db.set_mem_block_account_count(prev_txs_state.count().unpack())
            .unwrap();

        db.insert_block(
            genesis.clone(),
            Default::default(),
            global_state,
            Vec::new(),
            prev_txs_state,
            Vec::new(),
            Vec::new(),
        )
        .unwrap();

        let rollup_config: gw_types::packed::RollupConfig = config.rollup_config.to_owned().into();
        db.attach_block(genesis).unwrap();
        db.commit().unwrap();
    }
}
