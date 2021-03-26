use anyhow::{anyhow, Result};
use async_jsonrpc_client::HttpClient;
use ckb_types::prelude::Unpack as CKBUnpack;
use futures::{
    future::{join_all, select_all},
    FutureExt, TryFutureExt,
};
use gw_block_producer::{
    block_producer::{produce_block, ProduceBlockParam, ProduceBlockResult},
    types::{CellInfo, InputCellInfo},
};
use gw_chain::chain::Chain;
use gw_common::H256;
use gw_config::{BlockProducerConfig, Config};
use gw_generator::{
    account_lock_manage::AccountLockManage, backend_manage::BackendManage, genesis::init_genesis,
    Generator, RollupContext,
};
use gw_mem_pool::pool::MemPool;
use gw_store::Store;
use gw_types::{
    bytes::Bytes,
    core::{DepType, ScriptHashType},
    packed::{
        Byte32, CellDep, CellInput, CellOutput, CustodianLockArgs, DepositionLockArgs, GlobalState,
        L2Block, OutPoint, OutPointVec, Script, Transaction, WitnessArgs,
    },
    prelude::*,
};
use node::Node;
use parking_lot::Mutex;
use rpc_client::{DepositInfo, RPCClient};
use std::{
    collections::{HashMap, HashSet},
    convert::TryFrom,
    fs,
    path::Path,
    sync::Arc,
};
use transaction_skeleton::TransactionSkeleton;
use utils::fill_tx_fee;
use wallet::Wallet;

mod block_producer;
mod indexer_types;
mod node;
mod poller;
mod rpc_client;
mod transaction_skeleton;
mod utils;
mod wallet;

fn read_config<P: AsRef<Path>>(path: P) -> Result<Config> {
    let content = fs::read(path)?;
    let config = toml::from_slice(&content)?;
    Ok(config)
}

fn run() -> Result<()> {
    let config_path = "./config.toml";
    // read config
    let config = read_config(&config_path)?;

    let node = Node::from_config(config)?;
    smol::block_on(async { node.produce_next_block().await })?;

    Ok(())
}

/// Block producer
fn main() {
    run().expect("block producer");
}
