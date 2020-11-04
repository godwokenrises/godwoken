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

use anyhow::Result;
use chain::{Chain, HeaderInfo};
use ckb_types::prelude::*;
use collector::lumos::Lumos;
use collector::Collector;
use config::Config;
use consensus::{single_aggregator::SingleAggregator, traits::Consensus};
use crossbeam_channel::{bounded, RecvTimeoutError};
use gw_generator::Generator;
use parking_lot::Mutex;
use rpc::Server;
use state_impl::StateImpl;
use state_impl::SyncCodeStore;
use std::sync::Arc;
use std::time::Duration;
use tx_pool::TxPool;

fn build_config() -> Config {
    unimplemented!()
}

fn build_collector(_config: &Config) -> impl Collector {
    Lumos
}

fn run() -> Result<()> {
    let config = build_config();
    let consensus = SingleAggregator::new(config.consensus.aggregator_id);
    let tip = config.chain.l2_genesis.clone();
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
