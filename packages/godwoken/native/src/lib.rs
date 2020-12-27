use anyhow::Result;
use gw_chain::{
    chain::{Chain, ProduceBlockParam, ProduceBlockResult, SyncEvent, SyncParam},
    next_block_context::NextBlockContext,
    tx_pool::TxPool,
};
use gw_common::{state::State, H256};
use gw_config::{Config, GenesisConfig};
use gw_generator::{
    account_lock_manage::{always_success::AlwaysSuccess, AccountLockManage},
    backend_manage::{Backend, BackendManage},
    traits::CodeStore,
    Generator,
};
use gw_jsonrpc_types::{blockchain, genesis, parameter};
use gw_store::{genesis::build_genesis, Store};
use gw_types::{bytes::Bytes, core::Status, packed, prelude::*};
use neon::prelude::*;
use parking_lot::Mutex;
use std::sync::{Arc, RwLock};

pub struct NativeChain {
    pub config: Config,
    pub chain: Arc<RwLock<Chain>>,
}

fn build_generator() -> Generator {
    let mut backend_manage = BackendManage::default();
    let polyjuice_backend = {
        let validator = godwoken_polyjuice::BUNDLED_CELL
            .get("build/validator")
            .expect("get polyjuice validator binary");
        let generator = godwoken_polyjuice::BUNDLED_CELL
            .get("build/generator")
            .expect("get polyjuice generator binary");
        let validator_code_hash = godwoken_polyjuice::CODE_HASH_VALIDATOR.into();
        Backend {
            validator: Bytes::from(validator.into_owned()),
            generator: Bytes::from(generator.into_owned()),
            validator_code_hash,
        }
    };
    backend_manage.register_backend(polyjuice_backend);
    let mut account_lock_manage = AccountLockManage::default();
    let code_hash = H256::from([
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 1,
    ]);
    // TODO: add a real signature verifying implementation later
    account_lock_manage.register_lock_algorithm(code_hash, Box::new(AlwaysSuccess::default()));
    Generator::new(backend_manage, account_lock_manage)
}

declare_types! {
    pub class JsNativeChain for NativeChain {
        init(mut cx) {
            let config_string = cx.argument::<JsString>(0)?.value();
            let jsonrpc_config: parameter::Config = serde_json::from_str(&config_string).expect("Constructing config from string");
            let config: Config = jsonrpc_config.into();
            let js_header_info = cx.argument::<JsArrayBuffer>(1)?;
            let js_header_info_slice = cx.borrow(&js_header_info, |data| { data.as_slice::<u8>() });
            let header_info = packed::HeaderInfo::from_slice(js_header_info_slice).expect("Constructing header info");
            let mut store = Store::default();
            store.init_genesis(&config.genesis, header_info).expect("Initializing store");
            let tx_pool = {
                let nb_ctx = NextBlockContext {
                    aggregator_id: 0u32,
                    timestamp: 0u64,
                };
                let tip = packed::L2Block::default();
                let tx_pool = TxPool::create(
                    store.new_overlay().expect("State new overlay"), build_generator(),
                    &tip, nb_ctx).expect("Creating TxPool");
                Arc::new(Mutex::new(tx_pool))
            };
            let chain_result: Result<Chain> = Chain::create(
                config.clone().chain, store, build_generator(), Arc::clone(&tx_pool));
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
            let this = cx.this();
            let js_l2_transaction = cx.argument::<JsArrayBuffer>(0)?;
            let l2_transaction_slice = cx.borrow(&js_l2_transaction, |data| { data.as_slice::<u8>() });
            let l2_transaction = packed::L2Transaction::from_slice(l2_transaction_slice).expect("Build packed::L2Transaction from slice");
            let run_result: Result<gw_generator::RunResult > =
                cx.borrow(&this, |data| {
                    let chain = data.chain.write().unwrap();
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

        method submitWithdrawalRequest(mut cx) {
            let this = cx.this();
            let js_withdrawal_request = cx.argument::<JsArrayBuffer>(0)?;
            let withdrawal_request_slice = cx.borrow(&js_withdrawal_request, |data| { data.as_slice::<u8>() });
            let withdrawal_request = packed::WithdrawalRequest::from_slice(withdrawal_request_slice)
                .expect("Build packed::WithdrawalRequest from slice");
            let run_result: Result<()> =
                cx.borrow(&this, |data| {
                    let chain = data.chain.write().unwrap();
                    let result = chain.tx_pool.lock().push_withdrawal_request(withdrawal_request);
                    result
                });
            match run_result {
                Ok(()) => {
                    Ok(cx.undefined().upcast())
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

        method getBalance(mut cx) {
            let this = cx.this();
            let account_id = cx.argument::<JsNumber>(0)?.value() as u32;
            let sudt_id = cx.argument::<JsNumber>(1)?.value() as u32;
            let balance = cx.borrow(&this, |data| {
                let chain = data.chain.read().unwrap();
                chain.store.get_sudt_balance(sudt_id, account_id)
            });
            match balance {
                Ok(value) => {
                    let js_value = cx.string(format!("{:#x}", value));
                    Ok(js_value.upcast())
                },
                Err(e) => cx.throw_error(format!("GetBalance failed: {:?}", e))
            }
        }

        method getStorageAt(mut cx) {
            let this = cx.this();
            let account_id = cx.argument::<JsNumber>(0)?.value() as u32;
            let js_raw_key = cx.argument::<JsArrayBuffer>(1)?;
            let raw_key: H256 = cx.borrow(&js_raw_key, |data| {
                let data_slice = data.as_slice();
                let mut buf = [0u8; 32];
                buf.copy_from_slice(&data_slice[0..32]);
                H256::from(buf)
             });
            let get_raw_result = cx.borrow(&this, |data| {
                let chain = data.chain.read().unwrap();
                chain.store.get_value(account_id, &raw_key)
            });
            match get_raw_result {
                Ok(value) => {
                    let array: [u8; 32]= value.into();
                    let value =  packed::Byte32::from_slice(&array[0..32]).expect("Build packed::Byte32 from slice");
                    let js_value = cx.string(format!("{:#x}", value));
                    Ok(js_value.upcast())
                },
                Err(e) => cx.throw_error(format!("GetStoargeAt failed: {:?}", e))
            }
        }

        method getAccountIdByScriptHash(mut cx) {
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
                chain.store.get_account_id_by_script_hash(&raw_key)
            });
            match get_raw_result {
                Ok(Some(id)) => Ok(cx.number(id).upcast()),
                Ok(None) => Ok(cx.undefined().upcast()),
                Err(e) => cx.throw_error(format!("GetAccountIdByScript failed: {:?}", e))
            }
        }

        method getNonce(mut cx) {
            let this = cx.this();
            let account_id = cx.argument::<JsNumber>(0)?.value() as u32;
            let nonce = cx.borrow(&this, |data| {
                let chain = data.chain.read().unwrap();
                chain.store.get_nonce(account_id)
            });
            match nonce {
                Ok(value) => Ok(cx.number(value).upcast()),
                Err(e) => cx.throw_error(format!("GetNonce failed: {:?}", e))
            }
        }

        method getScriptHash(mut cx) {
            let this = cx.this();
            let account_id = cx.argument::<JsNumber>(0)?.value() as u32;
            let script_hash = cx.borrow(&this, |data| {
                let chain = data.chain.read().unwrap();
                chain.store.get_script_hash(account_id)
            });
            match script_hash {
                Ok(value) => {
                    let array: [u8; 32]= value.into();
                    let value =  packed::Byte32::from_slice(&array[0..32]).expect("Build packed::Byte32 from slice");
                    let js_value = cx.string(format!("{:#x}", value));
                    Ok(js_value.upcast())
                },
                Err(e) => cx.throw_error(format!("GetNonce failed: {:?}", e))
            }
        }

        method getScript(mut cx) {
            let this = cx.this();
            let js_raw_key = cx.argument::<JsArrayBuffer>(0)?;
            let raw_key: H256 = cx.borrow(&js_raw_key, |data| {
                let data_slice = data.as_slice();
                let mut buf = [0u8; 32];
                buf.copy_from_slice(&data_slice[0..32]);
                H256::from(buf)
             });
            let script = cx.borrow(&this, |data| {
                let chain = data.chain.read().unwrap();
                chain.store.get_script(&raw_key)
            });
            match script {
                Some(value) => {
                    let script_jsonrpc: blockchain::Script = value.into();
                    let script_string = serde_json::to_string(&script_jsonrpc).expect("Serializing Script");
                    Ok(cx.string(script_string).upcast())
                },
                None => Ok(cx.undefined().upcast())
            }
        }

        method getDataHash(mut cx) {
            let this = cx.this();
            let js_raw_key = cx.argument::<JsArrayBuffer>(0)?;
            let raw_key: H256 = cx.borrow(&js_raw_key, |data| {
                let data_slice = data.as_slice();
                let mut buf = [0u8; 32];
                buf.copy_from_slice(&data_slice[0..32]);
                H256::from(buf)
             });
            let data = cx.borrow(&this, |data| {
                let chain = data.chain.read().unwrap();
                chain.store.get_data_hash(&raw_key)
            });
            match data {
                Ok(value) => Ok(cx.boolean(value).upcast()),
                Err(e) => cx.throw_error(format!("GetDataHash failed: {:?}", e))
            }
        }

        method getData(mut cx) {
            let this = cx.this();
            let js_raw_key = cx.argument::<JsArrayBuffer>(0)?;
            let raw_key: H256 = cx.borrow(&js_raw_key, |data| {
                let data_slice = data.as_slice();
                let mut buf = [0u8; 32];
                buf.copy_from_slice(&data_slice[0..32]);
                H256::from(buf)
             });
            let data = cx.borrow(&this, |data| {
                let chain = data.chain.read().unwrap();
                chain.store.get_data(&raw_key)
            });
            match data {
                Some(value) => {
                    let js_value = cx.string(format!("{:#x}", value));
                    Ok(js_value.upcast())
                },
                None => Ok(cx.undefined().upcast())
            }
        }

        method tip(mut cx) {
            let this = cx.this();
            let l2_block: packed::L2Block=
                cx.borrow(&this, |data| {
                    let chain = data.chain.read().unwrap();
                    chain.local_state.tip().clone()
                });
            let l2_block_string = cx.string(format!("{:#x}", l2_block));
            Ok(l2_block_string.upcast())
        }

        method status(mut cx) {
            let this = cx.this();
            let status: Status =
                cx.borrow(&this, |data| {
                    let chain = data.chain.read().unwrap();
                    chain.local_state.status().clone()
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
    let genesis_state: genesis::GenesisWithGlobalState = genesis_state.into();
    let genesis_state_string =
        serde_json::to_string(&genesis_state).expect("serialize genesis config");
    Ok(cx.string(genesis_state_string))
}

register_module!(mut cx, {
    cx.export_class::<JsNativeChain>("NativeChain")?;
    cx.export_function("buildGenesisBlock", build_genesis_block)?;
    Ok(())
});
