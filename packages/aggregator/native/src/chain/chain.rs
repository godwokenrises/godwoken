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
    pub running: Arc<AtomicBool>,
    pub chain: Arc<RwLock<Chain>>,
}

impl NativeChain {
    pub fn running(&self) -> bool {
        self.running.load(Ordering::Acquire)
    }

    pub fn stop(&self) {
        self.running.store(false, Ordering::Release);
    }
}

declare_types! {
    pub class JsNativeChain for NativeChain {
        init(mut cx) {
            let config_path = cx.argument::<JsString>(0)?.value();
            let file = File::open(config_path).expect("Opening config file");
            //TODO: replace it with toml file
            let content: serde_json::Value = serde_json::from_reader(file).expect("Reading content from config file");
            let config: Config = serde_json::from_value(content).expect("Constructing config");
            let tip = genesis::build_genesis(&config.genesis).expect("Building genesis block from config");
            let last_synced = HeaderInfo {
                number: 0,
                block_hash: config.chain.genesis_block_hash,
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
                running: Arc::new(AtomicBool::new(false)),
                chain: Arc::new(RwLock::new(chain))
            })
        }

        method sync(mut cx) {
            let mut this = cx.this();
            let js_reverts_vec = cx.argument::<JsArray>(0)?.to_vec(&mut cx)?;
            let js_updates_vec = cx.argument::<JsArray>(1)?.to_vec(&mut cx)?;
            let js_next_block_context = cx.argument::<JsObject>(1)?;
            let mut reverts: Vec<L1Action> = vec![];
            let mut updates: Vec<L1Action> = vec![];
            for i in 0..js_reverts_vec.len() {
                let js_revert = js_reverts_vec[i as usize]
                    .downcast::<JsObject>()
                    .or_throw(&mut cx)?;
                // extract transaction_info
                let js_transaction_info = js_revert.get(&mut cx, "transaction_info")?
                    .downcast::<JsObject>()
                    .or_throw(&mut cx)?;
                let js_transaction = js_transaction_info.get(&mut cx, "transaction")?
                    .downcast::<JsArrayBuffer>()
                    .or_throw(&mut cx)?;
                let transaction = cx.borrow(&js_transaction, |data| {
                    let transaction_slice = data.as_slice::<u8>();
                    Transaction::from_slice(transaction_slice).expect("Building transaction from slice")
                });
                let js_block_hash = js_transaction_info.get(&mut cx, "block_hash")?
                    .downcast::<JsArrayBuffer>()
                    .or_throw(&mut cx)?;
                let block_hash = cx.borrow(&js_block_hash, |data| {
                    let mut block_hash = [0u8;32];
                    block_hash[..].copy_from_slice(data.as_slice::<u8>());
                    block_hash
                });
                let transaction_info = TransactionInfo {
                    transaction: transaction,
                    block_hash: block_hash
                };
                // extract header_info
                let js_header_info = js_revert.get(&mut cx, "header_info")?
                    .downcast::<JsObject>()
                    .or_throw(&mut cx)?;
                let number = js_header_info.get(&mut cx, "number")?
                    .downcast::<JsNumber>()
                    .or_throw(&mut cx)?
                    .value() as u64;
                let js_block_hash2 = js_header_info.get(&mut cx, "block_hash")?
                    .downcast::<JsArrayBuffer>()
                    .or_throw(&mut cx)?;
                let block_hash2 = cx.borrow(&js_block_hash2, |data| {
                    let mut block_hash = [0u8;32];
                    block_hash[..].copy_from_slice(data.as_slice::<u8>());
                    block_hash
                });
                let header_info = HeaderInfo {
                    number: number,
                    block_hash: block_hash2
                };
                // extract deposition_requests
                let js_deposition_requests = js_revert.get(&mut cx, "deposition_requests")?
                    .downcast::<JsArray>()
                    .or_throw(&mut cx)?
                    .to_vec(&mut cx)?;
                let mut deposition_requests: Vec<DepositionRequest> = vec![];
                for j in 0..js_deposition_requests.len() {
                    let js_deposition_request = js_deposition_requests[j as usize]
                        .downcast::<JsObject>()
                        .or_throw(&mut cx)?;
                    let js_script = js_deposition_request.get(&mut cx, "script")?
                        .downcast::<JsArrayBuffer>()
                        .or_throw(&mut cx)?;
                    let script = cx.borrow(&js_script, |data| {
                        let script_slice = data.as_slice::<u8>();
                        Script::from_slice(script_slice).expect("Building Script from slice")
                    });
                    let js_sudt_script = js_deposition_request.get(&mut cx, "sudt_script")?
                        .downcast::<JsArrayBuffer>()
                        .or_throw(&mut cx)?;
                    let sudt_script = cx.borrow(&js_sudt_script, |data| {
                        let script_slice = data.as_slice::<u8>();
                        Script::from_slice(script_slice).expect("Building Script from slice")
                    });
                    let amount = js_deposition_request.get(&mut cx, "amount")?
                        .downcast::<JsNumber>()
                        .or_throw(&mut cx)?
                        //TODO: update the value field type
                        .value() as u128;
                    let deposition_request = DepositionRequest {
                        script: script,
                        sudt_script: sudt_script,
                        amount: amount
                    };
                    deposition_requests.push(deposition_request)
                }
                let l1_action_context = L1ActionContext::SubmitTxs {
                    deposition_requests: deposition_requests,
                    withdrawal_requests: vec![],
                };
                let l1_action= L1Action {
                    transaction_info: transaction_info,
                    header_info: header_info,
                    context: l1_action_context,
                };
                reverts.push(l1_action);
            }
            for i in 0..js_updates_vec.len() {
                let js_update = js_updates_vec[i as usize]
                    .downcast::<JsObject>()
                    .or_throw(&mut cx)?;
                // extract transaction_info
                let js_transaction_info = js_update.get(&mut cx, "transaction_info")?
                    .downcast::<JsObject>()
                    .or_throw(&mut cx)?;
                let js_transaction = js_transaction_info.get(&mut cx, "transaction")?
                    .downcast::<JsArrayBuffer>()
                    .or_throw(&mut cx)?;
                let transaction = cx.borrow(&js_transaction, |data| {
                    let transaction_slice = data.as_slice::<u8>();
                    Transaction::from_slice(transaction_slice).expect("Building transaction from slice")
                });
                let js_block_hash = js_transaction_info.get(&mut cx, "block_hash")?
                    .downcast::<JsArrayBuffer>()
                    .or_throw(&mut cx)?;
                let block_hash = cx.borrow(&js_block_hash, |data| {
                    let mut block_hash = [0u8;32];
                    block_hash[..].copy_from_slice(data.as_slice::<u8>());
                    block_hash
                });
                let transaction_info = TransactionInfo {
                    transaction: transaction,
                    block_hash: block_hash
                };
                // extract header_info
                let js_header_info = js_update.get(&mut cx, "header_info")?
                    .downcast::<JsObject>()
                    .or_throw(&mut cx)?;
                let number = js_header_info.get(&mut cx, "number")?
                    .downcast::<JsNumber>()
                    .or_throw(&mut cx)?
                    .value() as u64;
                let js_block_hash2 = js_header_info.get(&mut cx, "block_hash")?
                    .downcast::<JsArrayBuffer>()
                    .or_throw(&mut cx)?;
                let block_hash2 = cx.borrow(&js_block_hash2, |data| {
                    let mut block_hash = [0u8;32];
                    block_hash[..].copy_from_slice(data.as_slice::<u8>());
                    block_hash
                });
                let header_info = HeaderInfo {
                    number: number,
                    block_hash: block_hash2
                };
                // extract deposition_requests
                let js_deposition_requests = js_update.get(&mut cx, "deposition_requests")?
                    .downcast::<JsArray>()
                    .or_throw(&mut cx)?
                    .to_vec(&mut cx)?;
                let mut deposition_requests: Vec<DepositionRequest> = vec![];
                for j in 0..js_deposition_requests.len() {
                    let js_deposition_request = js_deposition_requests[j as usize]
                        .downcast::<JsObject>()
                        .or_throw(&mut cx)?;
                    let js_script = js_deposition_request.get(&mut cx, "script")?
                        .downcast::<JsArrayBuffer>()
                        .or_throw(&mut cx)?;
                    let script = cx.borrow(&js_script, |data| {
                        let script_slice = data.as_slice::<u8>();
                        Script::from_slice(script_slice).expect("Building Script from slice")
                    });
                    let js_sudt_script = js_deposition_request.get(&mut cx, "sudt_script")?
                        .downcast::<JsArrayBuffer>()
                        .or_throw(&mut cx)?;
                    let sudt_script = cx.borrow(&js_sudt_script, |data| {
                        let script_slice = data.as_slice::<u8>();
                        Script::from_slice(script_slice).expect("Building Script from slice")
                    });
                    let amount = js_deposition_request.get(&mut cx, "amount")?
                        .downcast::<JsNumber>()
                        .or_throw(&mut cx)?
                        //TODO: update the value field type
                        .value() as u128;
                    let deposition_request = DepositionRequest {
                        script: script,
                        sudt_script: sudt_script,
                        amount: amount
                    };
                    deposition_requests.push(deposition_request)
                }
                let l1_action_context = L1ActionContext::SubmitTxs {
                    deposition_requests: deposition_requests,
                    withdrawal_requests: vec![],
                };
                let l1_action= L1Action {
                    transaction_info: transaction_info,
                    header_info: header_info,
                    context: l1_action_context,
                };
                updates.push(l1_action);
            }
            let aggregator_id = js_next_block_context.get(&mut cx, "aggregator_id")?
                .downcast::<JsNumber>()
                .or_throw(&mut cx)?.value() as u32;
            let timestamp = js_next_block_context.get(&mut cx, "timestamp")?
                .downcast::<JsNumber>()
                .or_throw(&mut cx)?.value() as u64;
            let next_block_context = NextBlockContext {
                aggregator_id: aggregator_id,
                timestamp: timestamp,
            };
            let sync_param = SyncParam {
                reverts: reverts,
                updates: updates,
                next_block_context: next_block_context,
            };
            cx.borrow_mut(&mut this, |data| {
                let mut chain = data.chain.write().unwrap();
                match chain.sync(sync_param) {
                    Ok(SyncEvent::Success) => {}
                    Ok(SyncEvent::BadBlock(start_challenge)) => {}
                    Ok(SyncEvent::WaitChallenge) => {}
                    Ok(SyncEvent::BadChallenge(cancel_challenge)) => {}
                    Err(error) => { println!("Unknown error: {:?}", error); }
                }
            });
            Ok(cx.undefined().upcast())
        }

        method produce_block(mut cx) {
            let mut this = cx.this();
            let aggregator_id = cx.argument::<JsNumber>(0)?.value() as u32;
            let js_deposition_requests_vec = cx.argument::<JsArray>(1)?.to_vec(&mut cx)?;
            let js_withdrawal_requests_vec = cx.argument::<JsArray>(2)?.to_vec(&mut cx)?;
            let mut deposition_requests: Vec<DepositionRequest> = vec![];
            for i in 0..js_deposition_requests_vec.len() {
                let js_deposition_request = js_deposition_requests_vec[i as usize]
                    .downcast::<JsObject>()
                    .or_throw(&mut cx)?;
                let js_script = js_deposition_request.get(&mut cx, "script")?
                    .downcast::<JsArrayBuffer>()
                    .or_throw(&mut cx)?;
                let script = cx.borrow(&js_script, |data| {
                    let script_slice = data.as_slice::<u8>();
                    Script::from_slice(script_slice).expect("Building Script from slice")
                });
                let js_sudt_script = js_deposition_request.get(&mut cx, "sudt_script")?
                    .downcast::<JsArrayBuffer>()
                    .or_throw(&mut cx)?;
                let sudt_script = cx.borrow(&js_sudt_script, |data| {
                    let script_slice = data.as_slice::<u8>();
                    Script::from_slice(script_slice).expect("Building Script from slice")
                });
                let amount= js_deposition_request.get(&mut cx, "amount")?
                    .downcast::<JsNumber>()
                    .or_throw(&mut cx)?
                    //TODO: update the value field type
                    .value() as u128;
                let deposition_request = DepositionRequest {
                    script: script,
                    sudt_script: sudt_script,
                    amount: amount
                };
                deposition_requests.push(deposition_request)
            }
            let mut withdrawal_requests: Vec<WithdrawalRequest> = vec![];
            for i in 0..js_withdrawal_requests_vec.len() {
                let js_withdrawal_request = js_withdrawal_requests_vec[i as usize]
                    .downcast::<JsObject>()
                    .or_throw(&mut cx)?;
                let js_lock_hash = js_withdrawal_request.get(&mut cx, "lock_hash")?
                    .downcast::<JsArrayBuffer>()
                    .or_throw(&mut cx)?;
                let lock_hash = cx.borrow(&js_lock_hash, |data| {
                    let mut hash = [0u8;32];
                    hash[..].copy_from_slice(data.as_slice::<u8>());
                    H256::from(hash)
                });
                let js_sudt_script_hash = js_withdrawal_request.get(&mut cx, "sudt_script_hash")?
                    .downcast::<JsArrayBuffer>()
                    .or_throw(&mut cx)?;
                let sudt_script_hash = cx.borrow(&js_sudt_script_hash, |data| {
                    let mut hash = [0u8;32];
                    hash[..].copy_from_slice(data.as_slice::<u8>());
                    H256::from(hash)
                });
                let amount = js_withdrawal_request.get(&mut cx, "amount")?
                    .downcast::<JsNumber>()
                    .or_throw(&mut cx)?
                    // TODO js number is f64, so need to use array buffer here
                    .value() as u128;
                let js_account_script_hash = js_withdrawal_request.get(&mut cx, "account_script_hash")?
                    .downcast::<JsArrayBuffer>()
                    .or_throw(&mut cx)?;
                let account_script_hash = cx.borrow(&js_account_script_hash, |data| {
                    let mut hash = [0u8;32];
                    hash[..].copy_from_slice(data.as_slice::<u8>());
                    H256::from(hash)
                });
                let withdrawal_request = WithdrawalRequest {
                    lock_hash: lock_hash,
                    sudt_script_hash: sudt_script_hash,
                    amount: amount,
                    account_script_hash: account_script_hash
                };
                withdrawal_requests.push(withdrawal_request);
            }
            let produce_block_param = ProduceBlockParam {
                aggregator_id: aggregator_id,
                deposition_requests: deposition_requests,
                withdrawal_requests: withdrawal_requests
            };
            cx.borrow_mut(&mut this, |data| {
                let mut chain = data.chain.write().unwrap();
                chain.produce_block(produce_block_param).expect("Syncing chain");
            });

            // TODO return L2BlockWithState
            Ok(cx.undefined().upcast())
        }

        method last_synced() {
            let this = cx.this();
            let header_info: HeaderInfo = cx.borrow(&this, |data| {
                let chain = data.chain.read().unwrap();
                chain.last_synced()
            });
            let js_header_info = JsObject::new(&mut cx);
            let js_block_number = cx.string(format!("{:#x}", header_info.number));
            js_header_info.set(&mut cx, "number", js_block_number)?;
            let js_block_hash = cx.string(format!("{:#x}", header_info.block_hash));
            js_header_info.set(&mut cx, "block_hash", js_block_hash)?;
            Ok(js_header_info.upcast())
        }

        method tip() {
            let this = cx.this();
            let tip: L2Block = cx.borrow(&this, |data| {
                let chain = data.chain.read().unwrap();
                chain.tip();
            })
            Ok(cx.undefined().upcast())
        }

        method status() {
            Ok(cx.undefined().upcast())
        }
    }
}
