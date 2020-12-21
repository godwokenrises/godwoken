use anyhow::Result;
use gw_chain::{
    chain::{Chain, ProduceBlockParam, ProduceBlockResult, Status, SyncEvent, SyncParam},
    next_block_context::NextBlockContext,
    tx_pool::TxPool,
};
use gw_config::Config;
use gw_generator::{
    account_lock_manage::AccountLockManage, backend_manage::BackendManage, Generator,
};
use gw_jsonrpc_types::parameter;
use gw_store::Store;
use gw_types::{packed, prelude::*};
use neon::prelude::*;
use parking_lot::Mutex;
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
            let jsonrpc_config: parameter::Config = serde_json::from_value(content).expect("Constructing config");
            let config: Config = jsonrpc_config.into();
            let store = Store::default();
            let tx_pool = {
                let generator = Generator::new(BackendManage::default(), AccountLockManage::default());
                let nb_ctx = NextBlockContext {
                    aggregator_id: 0u32,
                    timestamp: 0u64,
                };
                let tip = packed::L2Block::default();
                let tx_pool = TxPool::create(store.new_overlay().expect("State new overlay"), generator, &tip, nb_ctx).expect("Creating TxPool");
                Arc::new(Mutex::new(tx_pool))
            };
            let generator = Generator::new(BackendManage::default(), AccountLockManage::default());
            let chain_result: Result<Chain> = Chain::create(config.clone().chain, store, generator, Arc::clone(&tx_pool));
            match chain_result {
                Ok(chain) => Ok(NativeChain {
                    config: config,
                    chain: Arc::new(RwLock::new(chain))
                }),
                Err(e) => cx.throw_error(format!("Chain create failed: {:?}", e))
            }
        }

        method sync(mut cx) {
            let mut this = cx.this();
            let sync_param_string = cx.argument::<JsString>(0)?.value();
            let content: serde_json::Value = serde_json::from_str(&sync_param_string).expect("Reading from SyncParam string");
            let sync_param_jsonrpc: parameter::SyncParam = serde_json::from_value(content).expect("Constructing SyncParam");
            let sync_param: SyncParam = sync_param_jsonrpc.into();
            let sync_result: Result<SyncEvent> =
                cx.borrow_mut(&mut this, |data| {
                    let mut chain = data.chain.write().unwrap();
                    let sync_result = chain.sync(sync_param);
                    sync_result
                });
            match sync_result {
                Ok(sync_event) => {
                    let sync_event_jsonrpc: parameter::SyncEvent = sync_event.into();
                    let sync_event_string = serde_json::to_string(&sync_event_jsonrpc).expect("Serializing SyncEvent");
                    Ok(cx.string(sync_event_string).upcast())
                }
                Err(e) => cx.throw_error(format!("Chain sync failed: {:?}", e))
            }
        }

        method produceBlock(mut cx) {
            let mut this = cx.this();
            let produce_block_param_string = cx.argument::<JsString>(0)?.value();
            let content: serde_json::Value = serde_json::from_str(&produce_block_param_string).expect("Reading from ProduceBlockParam string");
            let produce_block_param_jsonrpc: parameter::ProduceBlockParam = serde_json::from_value(content).expect("Constructing ProduceBlockParam");
            let produce_block_param: ProduceBlockParam = produce_block_param_jsonrpc.into();
            let produce_block_result: Result<ProduceBlockResult> =
                cx.borrow_mut(&mut this, |data| {
                    let mut chain = data.chain.write().unwrap();
                    let produce_block_result = chain.produce_block(produce_block_param);
                    produce_block_result
                });
            match produce_block_result {
                Ok(produce_block_result) => {
                    let produce_block_result_jsonrpc: parameter::ProduceBlockResult= produce_block_result.into();
                    let produce_block_result_string = serde_json::to_string(&produce_block_result_jsonrpc).expect("Serializing L2BlockWithState");
                    Ok(cx.string(produce_block_result_string).upcast())
                }
                Err(e) => cx.throw_error(format!("Chain produce_block failed: {:?}", e))
            }
        }

        method execute(mut cx) {
            let this = cx.this();
            let js_l2_transaction = cx.argument::<JsArrayBuffer>(0)?;
            let l2_transaction_slice = cx.borrow(&js_l2_transaction, |data| { data.as_slice::<u8>() });
            let l2_transaction = packed::L2Transaction::from_slice(l2_transaction_slice).expect("Build packed::L2Transaction from slice");
            let run_result: Result<gw_generator::RunResult > =
                cx.borrow(&this, |data| {
                    data.chain.write().unwrap().tx_pool.lock().execute(l2_transaction)
                });
            match run_result {
                Ok(run_result) => {
                    let run_result_jsonrpc: parameter::RunResult = run_result.into();
                    let run_result_string = serde_json::to_string(&run_result_jsonrpc).expect("Serializing RunResult");
                    Ok(cx.string(run_result_string).upcast())
                }
                Err(e) => cx.throw_error(format!("Chain execute L2Transaction failed: {:?}", e))
            }
        }

        method submitL2Transaction(mut cx) {
            let mut this = cx.this();
            let js_l2_transaction = cx.argument::<JsArrayBuffer>(0)?;
            let l2_transaction_slice = cx.borrow(&js_l2_transaction, |data| { data.as_slice::<u8>() });
            let l2_transaction = packed::L2Transaction::from_slice(l2_transaction_slice).expect("Build packed::L2Transaction from slice");
            let run_result: Result<gw_generator::RunResult > =
                cx.borrow(&mut this, |data| {
                    let mut chain = data.chain.write().unwrap();
                    let run_result = chain.tx_pool.lock().push(l2_transaction);
                    run_result
                });
            match run_result {
                Ok(run_result) => {
                    let run_result_jsonrpc: parameter::RunResult = run_result.into();
                    let run_result_string = serde_json::to_string(&run_result_jsonrpc).expect("Serializing RunResult");
                    Ok(cx.string(run_result_string).upcast())
                }
                Err(e) => cx.throw_error(format!("Chain submit L2Transaction failed: {:?}", e))
            }
        }

        method lastSynced() {
            let this = cx.this();
            let header_info: HeaderInfo =
                cx.borrow(&this, |data| {
                    let chain = data.chain.read().unwrap();
                    chain.last_synced()
                });
            let header_info_jsonrpc: parameter::HeaderInfo = header_info.into();
            let header_info_string = serde_json::to_string(&header_info_jsonrpc).expect("Serializing HeaderInfo");
            Ok(cx.string(header_info_string).upcast())
        }

        method tip() {
            let this = cx.this();
            let l2_block: packed::L2Block=
                cx.borrow(&this, |data| {
                    let chain = data.chain.read().unwrap();
                    chain.tip()
                });
            let l2_block_jsonrpc: godwoken::L2Block = l2_block.into();
            let l2_block_string = serde_json::to_string(&l2_block_jsonrpc).expect("Serializing L2Block");
            Ok(cx.string(l2_block_string).upcast())
        }

        method status() {
            let this = cx.this();
            let status: Status =
                cx.borrow(&this, |data| {
                    let chain = data.chain.read().unwrap();
                    chain.status()
                });
            let status_jsonrpc: paramete::Status= status.into();
            let status_string = serde_json::to_string(&status_jsonrpc).expect("Serializing Status");
            Ok(cx.string(status_string).upcast())
        }
    }
}

register_module!(mut cx, {
    cx.export_class::<JsNativeChain>("NativeChain")?;
    Ok(())
});
