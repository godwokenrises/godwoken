//! Aggregator is an off-chain actor
//! Aggregator composite multiple components to process the off-chain status:
//!
//! * Sync layer2 blocks from layer1, then send to generator
//! * Accept layer2 tx via RPC, then send to generator
//! * Watch the chain, send challenge if a invalid block is committed
//! * Become validator and submit new blocks to layer1

mod chain;
mod collector;
mod config;
mod consensus;
mod crypto;
mod deposition;
mod jsonrpc_types;
mod rpc;
mod state_impl;
mod tx_pool;

use anyhow::{anyhow, Result};
use chain::{Chain, HeaderInfo};
use ckb_types::prelude::Unpack;
use collector::lumos::Lumos;
use collector::Collector;
use config::{Config, GenesisConfig};
use consensus::{single_aggregator::SingleAggregator, traits::Consensus};
use crossbeam_channel::{bounded, RecvTimeoutError};
use gw_common::{
    blake2b::new_blake2b,
    merkle_utils::serialize_block_key,
    smt::{default_store::DefaultStore, H256, SMT},
    state::{State, ZERO},
    CKB_TOKEN_ID,
};
use gw_generator::Generator;
use gw_types::{
    packed::{AccountMerkleState, L2Block, RawL2Block},
    prelude::{Builder as GWBuilder, Entity as GWEntity, Pack as GWPack},
};
use parking_lot::Mutex;
use rpc::Server;
use state_impl::StateImpl;
use state_impl::SyncCodeStore;
use std::sync::Arc;
use std::time::Duration;
use tx_pool::TxPool;

fn build_genesis(config: &GenesisConfig) -> Result<L2Block> {
    // build initialized states
    let mut state: StateImpl<DefaultStore<H256>> = Default::default();
    let root = state
        .calculate_root()
        .map_err(|err| anyhow!("calculate root error: {:?}", err))?;
    assert_eq!(root, ZERO, "initial root must be ZERO");

    // create a reserved account
    // this account is reserved for special use
    // for example: send a tx to reserved account to create a new contract account
    let reserved_account_id = state
        .create_account(ZERO, [0u8; 20])
        .map_err(|err| anyhow!("create reserved account error: {:?}", err))?;
    assert_eq!(reserved_account_id, 0, "reserved account id must be zero");

    // create initial aggregator
    let initial_aggregator_id = state
        .create_account(ZERO, config.initial_aggregator_pubkey.clone())
        .map_err(|err| anyhow!("create initial aggregator error: {:?}", err))?;
    state
        .mint_sudt(
            &CKB_TOKEN_ID,
            initial_aggregator_id,
            config.initial_deposition.into(),
        )
        .map_err(|err| anyhow!("mint sudt error: {:?}", err))?;

    // calculate post state
    let post_account = {
        let root = state
            .calculate_root()
            .map_err(|err| anyhow!("calculate root error: {:?}", err))?;
        let count = state
            .get_account_count()
            .map_err(|err| anyhow!("get account count error: {:?}", err))?;
        AccountMerkleState::new_builder()
            .merkle_root(root.pack())
            .count(count.pack())
            .build()
    };

    let raw_genesis = RawL2Block::new_builder()
        .number(0u64.pack())
        .aggregator_id(0u32.pack())
        .timestamp(config.timestamp.pack())
        .post_account(post_account)
        .valid(1.into())
        .build();

    // generate block proof
    let genesis_hash = {
        let mut buf = [0u8; 32];
        let mut hasher = new_blake2b();
        hasher.update(raw_genesis.as_slice());
        hasher.finalize(&mut buf);
        buf
    };
    let block_proof = {
        let block_key = serialize_block_key(0);
        let mut smt: SMT<DefaultStore<H256>> = Default::default();
        smt.update(block_key.into(), genesis_hash.into())
            .map_err(|err| anyhow!("update smt error: {:?}", err))?;
        smt.merkle_proof(vec![block_key.into()])
            .map_err(|err| anyhow!("gen merkle proof error: {:?}", err))?
            .compile(vec![(block_key.into(), genesis_hash.into())])
            .map_err(|err| anyhow!("compile merkle proof error: {:?}", err))?
    };

    // build genesis
    let genesis = L2Block::new_builder()
        .raw(raw_genesis)
        .block_proof(block_proof.0.pack())
        .build();
    Ok(genesis)
}

fn build_config() -> Config {
    unimplemented!()
}

fn build_collector(_config: &Config) -> impl Collector {
    Lumos
}

fn run() -> Result<()> {
    let config = build_config();
    let consensus = SingleAggregator::new(config.consensus.aggregator_id);
    let tip = build_genesis(&config.genesis)?;
    let collector = build_collector(&config);
    let genesis = collector.get_header_by_number(0).unwrap().unwrap();
    let last_synced = HeaderInfo {
        number: 0,
        block_hash: genesis.calc_header_hash().unpack(),
    };
    let code_store = SyncCodeStore::new(Default::default());
    let state = StateImpl::default();
    let tx_pool = {
        let generator = Generator::new(code_store.clone());
        let nb_ctx = consensus.next_block_context(&tip);
        let tx_pool = TxPool::create(state.new_overlay()?, generator, &tip, nb_ctx)?;
        Arc::new(Mutex::new(tx_pool))
    };
    let mut chain = {
        let generator = Generator::new(code_store);
        Chain::new(
            config.chain,
            state,
            consensus,
            tip,
            last_synced,
            collector,
            generator,
            Arc::clone(&tx_pool),
        )
    };
    println!("initial sync chain!");
    chain.sync()?;
    println!("start rpc server!");
    let (sync_tx, sync_rx) = bounded(1);
    let _server = Server::new()
        .enable_callback(sync_tx)
        .enable_tx_pool(Arc::clone(&tx_pool))
        .start("127.0.0.1:8080")?;

    // TODO support multiple aggregators and validator mode
    // We assume we are the only aggregator for now.
    loop {
        match sync_rx.recv_timeout(Duration::from_secs(10)) {
            Ok(()) => {
                // receive syncing notification
                println!("sync chain!");
                chain.sync()?;
            }
            Err(RecvTimeoutError::Timeout) => {
                // execute timeout event
            }
            Err(err) => panic!(err),
        }

        // TODO check tx pool to determine wether to produce a block or continue to collect more txs

        let deposition_requests = Vec::new();
        chain.produce_block(deposition_requests)?;
    }
}

fn main() {
    run().expect("run aggregator");
}
