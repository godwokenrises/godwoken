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
mod jsonrpc_types;
mod rpc;

use chain::{Chain, HeaderInfo};
use ckb_types::prelude::*;
use collector::lumos::Lumos;
use collector::Collector;
use config::Config;
use gw_generator::dummy_state::DummyState;
use gw_generator::syscalls::hashmap_code_store::HashMapCodeStore;

fn build_config() -> Config {
    unimplemented!()
}

fn build_collector(_config: &Config) -> impl Collector {
    Lumos
}

fn main() {
    let config = build_config();
    let state = DummyState::default();
    let tip = config.rollup.l2_genesis.clone();
    let collector = build_collector(&config);
    let genesis = collector.get_header_by_number(0).unwrap().unwrap();
    let last_synced = HeaderInfo {
        number: 0,
        block_hash: genesis.calc_header_hash().unpack(),
    };
    let code_store = HashMapCodeStore::new(Default::default());
    let rollup_type_script = config.rollup.rollup_type_script.clone();
    let mut chain = Chain::new(
        state,
        tip,
        last_synced,
        rollup_type_script,
        collector,
        code_store,
    );
    println!("sync chain!");
    chain.sync().expect("sync");
}
