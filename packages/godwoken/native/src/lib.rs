use ckb_types::packed::Transaction;
use gw_chain::{
    chain::{
        Chain, HeaderInfo, L1Action, L1ActionContext, ProduceBlockParam, SyncEvent, SyncParam,
        TransactionInfo,
    },
    genesis,
    next_block_context::NextBlockContext,
    tx_pool::TxPool,
};
use gw_common::smt::{H256, SMT};
use gw_common::sparse_merkle_tree::SparseMerkleTree;
use gw_config::{Config, GenesisConfig};
use gw_generator::{
    backend_manage::BackendManage,
    generator::{DepositionRequest, WithdrawalRequest},
    Generator,
};
use gw_store::Store;
use gw_types::{
    bytes::Bytes,
    packed::{L2Block, L2Transaction, RawL2Block, Script},
    prelude::*,
};
use neon::prelude::*;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::fs::File;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

pub struct NativeChain {
    pub config: Config,
    // Deprecated
    pub chain: Arc<RwLock<Chain>>,
}

declare_types! {
    pub class JsNativeChain for NativeChain {
        init(mut cx) {
            let config_string = cx.argument::<JsString>(0)?.value();
            let content: serde_json::Value = serde_json::from_str(&config_string).expect("Reading from config string");
            let config: Config = serde_json::from_value(content).expect("Constructing config");
            let tip = genesis::build_genesis(&config.genesis).expect("Building genesis block from config");
            let last_synced = HeaderInfo {
                number: 0,
                block_hash: [08;32],
            };
            let store = Store::default();
            let tx_pool = {
                let generator = Generator::new(BackendManage::default());
                let nb_ctx = NextBlockContext {
                    aggregator_id: 0u32,
                    timestamp: 0u64,
                };
                let tx_pool = TxPool::create(store.new_overlay().expect("State new overlay"), generator, &tip, nb_ctx).expect("Creating TxPool");
                Arc::new(Mutex::new(tx_pool))
            };
            let chain = {
                let generator = Generator::new(BackendManage::default());
                Chain::new(
                    config.clone().chain,
                    store,
                    tip,
                    last_synced,
                    generator,
                    Arc::clone(&tx_pool),
                )
            };

            Ok(NativeChain {
                config: config,
                chain: Arc::new(RwLock::new(chain))
            })
        }

        method sync(mut cx) {
            let mut this = cx.this();
            Ok(cx.undefined().upcast())
        }

        method produce_block(mut cx) {
            let mut this = cx.this();
            Ok(cx.undefined().upcast())
        }

        method last_synced() {
            let this = cx.this();
            Ok(cx.undefined().upcast())
        }

        method tip() {
            let this = cx.this();
            Ok(cx.undefined().upcast())
        }

        method status() {
            Ok(cx.undefined().upcast())
        }
    }
}

register_module!(mut cx, {
    cx.export_class::<JsNativeChain>("NativeChain")?;
    Ok(())
});
