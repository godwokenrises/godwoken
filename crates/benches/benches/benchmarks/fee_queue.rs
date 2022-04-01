use criterion::{criterion_group, Bencher, Criterion};
use gw_common::{h256_ext::H256Ext, state::State, H256};
use gw_config::GenesisConfig;
use gw_generator::genesis::init_genesis;
use gw_mem_pool::fee::{
    queue::FeeQueue,
    types::{FeeEntry, FeeItem},
};
use gw_store::{
    mem_pool_state::MemStore, state::state_db::StateContext, traits::chain_store::ChainStore, Store,
};
use gw_types::{
    bytes::Bytes,
    packed::{
        AllowedTypeHash, L2BlockCommittedInfo, L2Transaction, RawL2Transaction, RollupConfig,
    },
    prelude::{Builder, Entity, Pack, PackVec, Unpack},
};

const MAX_QUEUE_SIZE: usize = 100_000;

fn bench_add_full(b: &mut Bencher) {
    let mut queue = FeeQueue::new();

    let store = Store::open_tmp().expect("open store");
    setup_genesis(&store);
    {
        let db = store.begin_transaction();
        let genesis = db.get_tip_block().expect("tip");
        assert_eq!(genesis.raw().number().unpack(), 0);
        let mut state = db.state_tree(StateContext::AttachBlock(1)).expect("state");

        // create accounts
        for i in 0..4 {
            state.create_account(H256::from_u32(i)).unwrap();
        }

        db.commit().expect("commit");
    }

    for i in 0..(MAX_QUEUE_SIZE as u32) {
        let entry1 = FeeEntry {
            item: FeeItem::Tx(
                L2Transaction::new_builder()
                    .raw(RawL2Transaction::new_builder().nonce(i.pack()).build())
                    .build(),
            ),
            fee: 100 * 1000,
            cycles_limit: 1000,
            sender: 2,
            order: queue.len(),
        };
        queue.add(entry1);
    }

    assert_eq!(queue.len(), MAX_QUEUE_SIZE);

    b.iter(|| {
        let entry1 = FeeEntry {
            item: FeeItem::Tx(
                L2Transaction::new_builder()
                    .raw(
                        RawL2Transaction::new_builder()
                            .nonce(10001u32.pack())
                            .build(),
                    )
                    .build(),
            ),
            fee: 100 * 1000,
            cycles_limit: 1000,
            sender: 2,
            order: queue.len(),
        };
        queue.add(entry1);
    });
}

fn bench_add_fetch_20(b: &mut Bencher) {
    let mut queue = FeeQueue::new();

    let store = Store::open_tmp().expect("open store");
    setup_genesis(&store);
    {
        let db = store.begin_transaction();
        let genesis = db.get_tip_block().expect("tip");
        assert_eq!(genesis.raw().number().unpack(), 0);
        let mut state = db.state_tree(StateContext::AttachBlock(1)).expect("state");

        // create accounts
        for i in 0..4 {
            state.create_account(H256::from_u32(i)).unwrap();
        }

        db.commit().expect("commit");
    }
    let snap = store.get_snapshot();

    for i in 0..(MAX_QUEUE_SIZE as u32) - 100 {
        let entry1 = FeeEntry {
            item: FeeItem::Tx(
                L2Transaction::new_builder()
                    .raw(RawL2Transaction::new_builder().nonce(i.pack()).build())
                    .build(),
            ),
            fee: 100 * 1000,
            cycles_limit: 1000,
            sender: 2,
            order: queue.len(),
        };
        queue.add(entry1);
    }

    let mem_store = MemStore::new(snap);
    let tree = mem_store.state().unwrap();

    b.iter(|| {
        for i in 0..20 {
            let entry1 = FeeEntry {
                item: FeeItem::Tx(
                    L2Transaction::new_builder()
                        .raw(
                            RawL2Transaction::new_builder()
                                .nonce((MAX_QUEUE_SIZE + i).pack())
                                .build(),
                        )
                        .build(),
                ),
                fee: 100 * 1000,
                cycles_limit: 1000,
                sender: 2,
                order: queue.len(),
            };
            queue.add(entry1);
        }
        queue.fetch(&tree, 20)
    });
}

pub fn bench(c: &mut Criterion) {
    c.bench_function("FeeQueue add when full", |b| {
        bench_add_full(b);
    });
    c.bench_function("FeeQueue add and fetch 20", |b| {
        bench_add_fetch_20(b);
    });
}

criterion_group! {
    name = fee_queue;
    config = Criterion::default().sample_size(10);
    targets = bench
}

const ALWAYS_SUCCESS_CODE_HASH: [u8; 32] = [42u8; 32];

fn setup_genesis(store: &Store) {
    let rollup_type_hash = H256::from_u32(42);
    let rollup_config = RollupConfig::new_builder()
        .allowed_eoa_type_hashes(
            vec![AllowedTypeHash::from_unknown(ALWAYS_SUCCESS_CODE_HASH)].pack(),
        )
        .finality_blocks(0.pack())
        .build();
    let genesis_config = GenesisConfig {
        timestamp: 0,
        meta_contract_validator_type_hash: Default::default(),
        rollup_config: rollup_config.into(),
        rollup_type_hash: {
            let h: [u8; 32] = rollup_type_hash.into();
            h.into()
        },
        secp_data_dep: Default::default(),
    };
    let genesis_committed_info = L2BlockCommittedInfo::default();
    init_genesis(
        store,
        &genesis_config,
        genesis_committed_info,
        Bytes::default(),
    )
    .unwrap();
}
