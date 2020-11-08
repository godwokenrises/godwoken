//! Aggregator is an off-chain actor
//! Aggregator composite multiple components to process the off-chain status:
//!
//! * Sync layer2 blocks from layer1, then send to generator
//! * Accept layer2 tx via RPC, then send to generator
//! * Watch the chain, send challenge if a invalid block is committed
//! * Become validator and submit new blocks to layer1

mod chain;
mod consensus;
mod crypto;
mod genesis;
mod rpc;
mod state_impl;
mod tx_pool;

use anyhow::{anyhow, Result};
use chain::{Chain, HeaderInfo, ProduceBlockParam};
use consensus::{single_aggregator::SingleAggregator, traits::Consensus};
use crossbeam_channel::{bounded, RecvTimeoutError};
use genesis::build_genesis;
use gw_config::Config;
use gw_generator::Generator;
use parking_lot::Mutex;
use rpc::Server;
use state_impl::StateImpl;
use state_impl::SyncCodeStore;
use std::fs;
use std::sync::Arc;
use std::time::Duration;
use tx_pool::TxPool;

fn parse_config(path: &str) -> Result<Config> {
    let content = fs::read(path)?;
    let config: Config = toml::from_slice(&content)?;
    Ok(config)
}

fn run() -> Result<()> {
    let config_path = std::env::args().skip(1).next().expect("config file path");
    let config = parse_config(&config_path)?;
    let consensus = SingleAggregator::new(config.consensus.aggregator_id);
    let tip = build_genesis(&config.genesis)?;
    let genesis = unreachable!();
    let last_synced = HeaderInfo {
        number: 0,
        block_hash: unimplemented!(),
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
            generator,
            Arc::clone(&tx_pool),
        )
    };
    let aggregator_id = config
        .aggregator
        .as_ref()
        .ok_or(anyhow!("aggregator is not configured!"))?
        .account_id;
    println!("initial sync chain!");
    let sync_param = unimplemented!();
    chain.sync(sync_param)?;
    println!("start rpc server!");
    let (sync_tx, sync_rx) = bounded(1);
    let _server = Server::new()
        .enable_callback(sync_tx)
        .enable_tx_pool(Arc::clone(&tx_pool))
        .start(&config.rpc.listen)?;

    // TODO support multiple aggregators and validator mode
    // We assume we are the only aggregator for now.
    loop {
        match sync_rx.recv_timeout(Duration::from_secs(10)) {
            Ok(()) => {
                // receive syncing notification
                println!("sync chain!");
                let sync_param = unimplemented!();
                chain.sync(sync_param)?;
            }
            Err(RecvTimeoutError::Timeout) => {
                // execute timeout event
            }
            Err(err) => panic!(err),
        }

        // TODO check tx pool to determine wether to produce a block or continue to collect more txs

        let deposition_requests = Vec::new();
        let block = chain.produce_block(ProduceBlockParam {
            aggregator_id,
            deposition_requests,
        })?;
        // signer.sign(block)
        // client.commit_block(block);
    }
}

fn main() {
    run().expect("run aggregator");
}
