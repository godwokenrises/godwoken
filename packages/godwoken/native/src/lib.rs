use anyhow::Result;
use gw_chain::{
    chain::{Chain, ProduceBlockParam, ProduceBlockResult, Status, SyncEvent, SyncParam},
    next_block_context::NextBlockContext,
    tx_pool::TxPool,
};
use gw_config::{Config, GenesisConfig};
use gw_generator::{
    account_lock_manage::AccountLockManage, backend_manage::BackendManage, Generator,
};
use gw_jsonrpc_types::{genesis, parameter};
use gw_store::{
    genesis::{build_genesis, GenesisWithSMTState},
    Store,
};
use gw_types::{packed, prelude::*};
use neon::prelude::*;
use parking_lot::Mutex;
use std::sync::{Arc, RwLock};

pub struct NativeChain {
    pub config: Config,
    pub chain: Arc<RwLock<Chain>>,
}

declare_types! {
    pub class JsNativeChain for NativeChain {
        init(mut cx) {
            let config_string = cx.argument::<JsString>(0)?.value();
            let jsonrpc_config: parameter::Config = serde_json::from_str(&config_string).expect("Constructing config from string");
            let config: Config = jsonrpc_config.into();
            let genesis_setup_string = cx.argument::<JsString>(1)?.value();
            let genesis_setup: genesis::GenesisSetup = serde_json::from_str(&genesis_setup_string).expect("Construcing genesis setup from string");
            let genesis_with_smt: GenesisWithSMTState = genesis_setup.genesis.into();
            let header_info = packed::HeaderInfo::from_slice(genesis_setup.header_info.into_bytes().as_ref()).expect("Constructing header info");
            let mut store = Store::default();
            println!("Initializing store!");
            store.init_genesis(genesis_with_smt, header_info).expect("Initializing store");
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
            let sync_param_jsonrpc: parameter::SyncParam = serde_json::from_str(&sync_param_string).expect("Constructing SyncParam from string");
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
            let produce_block_param_jsonrpc: parameter::ProduceBlockParam = serde_json::from_str(&produce_block_param_string).expect("Constructing ProduceBlockParam from string");
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

        method lastSynced(mut cx) {
            let this = cx.this();
            let header_info: packed::HeaderInfo =
                cx.borrow(&this, |data| {
                    let chain = data.chain.read().unwrap();
                    chain.local_state.last_synced().clone()
                });
            let js_value = cx.string(format!("{:#x}", header_info));
            Ok(js_value.upcast())
        }

        method getStorageAt() {
            let this = cx.this();
            let js_raw_key = cx.argument::<JsArrayBuffer>(0)?;
            let raw_key: H256 = cx.borrow(&js_raw_key, |data| {
                let data_slice = data.as_slice();
                let mut buf = [0u8; 32];
                buf.copy_from_slice(&data_slice[0..32]);
                H256::from(buf)
             });
            let get_raw_result = cx.borrow(&this, |data| {
                let chain = data.chain.read().unwrap();
                chain.store.get_raw(raw_key);
            });
            match get_raw_result {
                Ok(value) => {
                    let array: [u8; 32]= value.into();
                    let value =  packed::Byte32::from_slice(slice: &array[0..32]).expect("Build packed::Byte32 from slice");
                    let js_value = cx.string(format!("{:#x}", value));
                    Ok(js_value.upcast())
                },
                Err(e) => cx.throw_error(format!("GetStoargeAt failed: {:?}", e))
            }
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
            let status_jsonrpc: parameter::Status= status.into();
            let status_string = serde_json::to_string(&status_jsonrpc).expect("Serializing Status");
            Ok(cx.string(status_string).upcast())
        }
    }
}

pub fn build_genesis_block(mut cx: FunctionContext) -> JsResult<JsString> {
    let genesis_config = cx.argument::<JsString>(0)?.value();
    let genesis_config: parameter::GenesisConfig =
        serde_json::from_str(&genesis_config).expect("Parse genesis config");
    let genesis_config: GenesisConfig = genesis_config.into();
    let genesis_state = build_genesis(&genesis_config).expect("build genesis");
    let genesis_state: genesis::GenesisWithSMTState = genesis_state.into();
    let genesis_state_string =
        serde_json::to_string(&genesis_state).expect("serialize genesis config");
    Ok(cx.string(genesis_state_string))
}

register_module!(mut cx, {
    cx.export_class::<JsNativeChain>("NativeChain")?;
    cx.export_function("buildGenesisBlock", build_genesis_block)?;
    Ok(())
});
