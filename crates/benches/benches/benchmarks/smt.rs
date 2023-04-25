use std::sync::Arc;

use anyhow::Result;
use criterion::{criterion_group, BenchmarkId, Criterion, Throughput};
use gw_builtin_binaries::{file_checksum, Resource};
use gw_common::{
    blake2b::new_blake2b,
    builtins::{CKB_SUDT_ACCOUNT_ID, ETH_REGISTRY_ACCOUNT_ID},
    registry_address::RegistryAddress,
    state::State,
};
use gw_config::{BackendConfig, BackendForkConfig, GenesisConfig, StoreConfig};
use gw_generator::{
    account_lock_manage::{always_success::AlwaysSuccess, AccountLockManage},
    backend_manage::BackendManage,
    genesis::build_genesis_from_store,
    traits::StateExt,
    Generator,
};
use gw_store::{
    mem_pool_state::MemPoolState,
    schema::COLUMNS,
    state::{
        history::history_state::{HistoryState, RWConfig},
        state_db::StateDB,
        traits::JournalDB,
        MemStateDB,
    },
    traits::chain_store::ChainStore,
    Store,
};
use gw_traits::{ChainView, CodeStore};
use gw_types::{
    bytes::Bytes,
    core::{AllowedEoaType, ScriptHashType, Status},
    h256::*,
    packed::{
        AccountMerkleState, AllowedTypeHash, BlockInfo, BlockMerkleState, Fee, GlobalState,
        L2Block, RawL2Block, RawL2Transaction, RollupConfig, SUDTArgs, SUDTTransfer, Script,
        SubmitTransactions,
    },
    prelude::*,
    U256,
};
use gw_utils::RollupContext;
use pprof::criterion::{Output, PProfProfiler};

// meta contract
const META_GENERATOR_PATH: &str =
    "../../crates/builtin-binaries/builtin/gwos-v1.3.0-rc1/meta-contract-generator";
const META_VALIDATOR_SCRIPT_TYPE_HASH: [u8; 32] = [1u8; 32];

// sudt contract
const SUDT_GENERATOR_PATH: &str =
    "../../crates/builtin-binaries/builtin/gwos-v1.3.0-rc1/sudt-generator";
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
    };
    let store = Store::open(&config, COLUMNS).unwrap();
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
    fn build_script(n: u32) -> (Script, RegistryAddress) {
        let mut addr = [0u8; 20];
        addr[..4].copy_from_slice(&n.to_le_bytes());
        let mut args = vec![42u8; 32];
        args.extend(&addr);
        let script = Script::new_builder()
            .code_hash(ALWAYS_SUCCESS_LOCK_HASH.pack())
            .hash_type(ScriptHashType::Type.into())
            .args(args.pack())
            .build();
        let addr = RegistryAddress::new(ETH_REGISTRY_ACCOUNT_ID, addr.to_vec());
        (script, addr)
    }
}

struct BenchChain;
impl ChainView for BenchChain {
    fn get_block_hash_by_number(&self, _: u64) -> Result<Option<H256>> {
        unreachable!("bench chain store")
    }
}

struct BenchExecutionEnvironment {
    generator: Generator,
    chain: BenchChain,
    mem_pool_state: MemPoolState,
}

impl BenchExecutionEnvironment {
    fn new_with_accounts(store: Store, accounts: u32) -> Self {
        let genesis_config = GenesisConfig {
            meta_contract_validator_type_hash: META_VALIDATOR_SCRIPT_TYPE_HASH.into(),
            rollup_type_hash: ROLLUP_TYPE_HASH.into(),
            rollup_config: RollupConfig::new_builder()
                .l2_sudt_validator_script_type_hash(SUDT_VALIDATOR_SCRIPT_TYPE_HASH.pack())
                .allowed_eoa_type_hashes(
                    vec![AllowedTypeHash::new_builder()
                        .hash(ALWAYS_SUCCESS_LOCK_HASH.pack())
                        .type_(AllowedEoaType::Eth.into())
                        .build()]
                    .pack(),
                )
                .build()
                .into(),
            ..Default::default()
        };

        let rollup_context = RollupContext {
            rollup_config: genesis_config.rollup_config.clone().into(),
            rollup_script_hash: ROLLUP_TYPE_HASH,
            ..Default::default()
        };

        let backend_manage = {
            let configs = vec![
                BackendConfig {
                    generator: Resource::file_system(META_GENERATOR_PATH.into()),
                    generator_checksum: file_checksum(&META_GENERATOR_PATH).unwrap().into(),
                    validator_script_type_hash: META_VALIDATOR_SCRIPT_TYPE_HASH.into(),
                    backend_type: gw_config::BackendType::Meta,
                    generator_debug: None,
                },
                BackendConfig {
                    generator: Resource::file_system(SUDT_GENERATOR_PATH.into()),
                    generator_checksum: file_checksum(&SUDT_GENERATOR_PATH).unwrap().into(),
                    validator_script_type_hash: SUDT_VALIDATOR_SCRIPT_TYPE_HASH.into(),
                    backend_type: gw_config::BackendType::Sudt,
                    generator_debug: None,
                },
            ];
            BackendManage::from_config(vec![BackendForkConfig {
                sudt_proxy: Default::default(),
                fork_height: 0,
                backends: configs,
            }])
            .expect("bench backend")
        };

        let account_lock_manage = {
            let mut manage = AccountLockManage::default();
            manage.register_lock_algorithm(ALWAYS_SUCCESS_LOCK_HASH, Arc::new(AlwaysSuccess));
            manage
        };

        let generator = Generator::new(
            backend_manage,
            account_lock_manage,
            rollup_context,
            Default::default(),
        );

        Self::init_genesis(&store, &genesis_config, accounts);
        let mem_pool_state = MemPoolState::new(
            MemStateDB::from_store(store.get_snapshot()).expect("mem state db"),
            true,
        );

        BenchExecutionEnvironment {
            generator,
            chain: BenchChain,
            mem_pool_state,
        }
    }

    fn accounts_transfer(&self, accounts: u32, count: usize) {
        let mut state = self.mem_pool_state.load_state_db();

        let (block_producer_script, block_producer) = Account::build_script(0);
        let block_info = BlockInfo::new_builder()
            .block_producer(Bytes::from(block_producer.to_bytes()).pack())
            .number(1.pack())
            .timestamp(1.pack())
            .build();

        let block_producer_balance = state
            .get_sudt_balance(CKB_SUDT_ACCOUNT_ID, &block_producer)
            .unwrap();

        let addrs: Vec<_> = (0..=accounts)
            .map(Account::build_script)
            .map(|(_s, addr)| addr)
            .collect();

        let address_offset = state
            .get_account_id_by_script_hash(&block_producer_script.hash())
            .unwrap()
            .unwrap(); // start from block producer
        let start_account_id = address_offset + 1;
        let end_account_id = address_offset + accounts;

        // Loop transfer from id to id + 1, until we reach target count
        let mut from_id = start_account_id;
        let mut transfer_count = count;
        while transfer_count > 0 {
            let to_address = {
                let mut to_id = from_id + 1;
                if to_id > end_account_id {
                    to_id = start_account_id;
                }
                addrs.get((to_id - address_offset) as usize).unwrap()
            };

            let args = SUDTArgs::new_builder()
                .set(
                    SUDTTransfer::new_builder()
                        .to_address(Bytes::from(to_address.to_bytes()).pack())
                        .amount(U256::one().pack())
                        .fee(
                            Fee::new_builder()
                                .registry_id(ETH_REGISTRY_ACCOUNT_ID.pack())
                                .amount(1u128.pack())
                                .build(),
                        )
                        .build(),
                )
                .build();

            let raw_tx = RawL2Transaction::new_builder()
                .from_id(from_id.pack())
                .to_id(1u32.pack())
                .args(args.as_bytes().pack())
                .build();

            self.generator
                .execute_transaction(
                    &self.chain,
                    &mut state,
                    &block_info,
                    &raw_tx,
                    Some(u64::MAX),
                    None,
                )
                .unwrap();

            state.finalise().unwrap();

            from_id += 1;
            if from_id > end_account_id {
                from_id = start_account_id;
            }
            transfer_count -= 1;
        }
        self.mem_pool_state.store_state_db(state);

        let state = self.mem_pool_state.load_state_db();
        let post_block_producer_balance = state
            .get_sudt_balance(CKB_SUDT_ACCOUNT_ID, &block_producer)
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
            let (account_script, addr) = Account::build_script(idx);
            let account_script_hash: H256 = account_script.hash();
            let account_id = state.create_account(account_script_hash).unwrap();
            state.insert_script(account_script_hash, account_script);
            state
                .mapping_registry_address_to_script_hash(addr.clone(), account_script_hash)
                .unwrap();
            state
                .mint_sudt(CKB_SUDT_ACCOUNT_ID, &addr, CKB_BALANCE.into())
                .unwrap();

            Account { id: account_id }
        };

        (0..accounts).map(build_account).collect()
    }

    fn init_genesis(store: &Store, config: &GenesisConfig, accounts: u32) {
        if store.has_genesis().unwrap() {
            let chain_id = store.get_chain_id().unwrap();
            if chain_id == ROLLUP_TYPE_HASH {
                return;
            } else {
                panic!("store genesis already initialized");
            }
        }

        let mut db = store.begin_transaction();
        db.setup_chain_id(ROLLUP_TYPE_HASH).unwrap();
        let (mut db, genesis_state) =
            build_genesis_from_store(db, config, Default::default()).unwrap();

        let smt = db
            .state_smt_with_merkle_state(genesis_state.genesis.raw().post_account())
            .unwrap();
        let account_count = genesis_state.genesis.raw().post_account().count().unpack();
        let mut state = {
            let history_state = HistoryState::new(smt, account_count, RWConfig::attach_block(0));
            StateDB::new(history_state)
        };

        Self::generate_accounts(&mut state, accounts + 1); // Plus block producer
        state.finalise().unwrap();

        let (genesis, global_state) = {
            let prev_state_checkpoint: [u8; 32] = state.calculate_state_checkpoint().unwrap();
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
                    .compile(vec![block_key.into()])
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
        db.insert_block(
            genesis.clone(),
            global_state,
            prev_txs_state,
            Vec::new(),
            Default::default(),
            Vec::new(),
        )
        .unwrap();

        db.attach_block(genesis).unwrap();
        db.commit().unwrap();
    }
}
