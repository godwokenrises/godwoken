use anyhow::{anyhow, Result};
use ckb_types::{
    bytes::Bytes,
    packed::{RawTransaction, Script, Transaction, WitnessArgs, WitnessArgsReader},
    prelude::Unpack,
};
use gw_chain::{
    chain::{Chain, HeaderInfo, ProduceBlockParam, SyncInfo, SyncParam, TransactionInfo},
    consensus::{single_aggregator::SingleAggregator, traits::Consensus},
    genesis,
    rpc::Server,
    state_impl::{StateImpl, SyncCodeStore},
    tx_pool::TxPool,
};
use gw_config::{Config, GenesisConfig};
use gw_generator::{generator::DepositionRequest, Generator, HashMapCodeStore};
use gw_types::{
    packed::{AccountMerkleState, L2Block, RawL2Block},
    prelude::*,
};
use neon::prelude::*;
use parking_lot::Mutex;
use std::convert::TryInto;
use std::fs::File;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

pub struct NativeChain {
    pub config: Config,
    pub running: Arc<AtomicBool>,
    pub chain: Arc<Chain<SyncCodeStore, SingleAggregator>>,
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
            let configPath = cx.argument::<JsString>(0)?.value();
            let file = File::open(configPath).expect("Opening config file");
            //TODO: replace it with toml file
            let content: serde_json::Value = serde_json::from_reader(file).expect("Reading content from config file");
            let config: Config = serde_json::from_value(content).expect("Constructing config");
            let consensus = SingleAggregator::new(config.consensus.aggregator_id);
            let tip = genesis::build_genesis(&config.genesis).expect("Building genesis block from config");
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
                let tx_pool = TxPool::create(state.new_overlay().expect("State new overlay"), generator, &tip, nb_ctx).expect("Creating TxPool");
                Arc::new(Mutex::new(tx_pool))
            };
            let chain = {
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

            Ok(NativeChain {
                config: config,
                running: Arc::new(AtomicBool::new(false)),
                chain: Arc::new(chain)
            })
        }

        method start_rpc_server(mut cx) {
            let this = cx.this();
            let config = cx.borrow(&this, |data| { data.config.clone() });
            let tx_pool = cx.borrow(&this, |data| { data.chain.tx_pool().clone() });
            Server::new()
                .enable_tx_pool(tx_pool)
                .start(&config.rpc.listen).expect("Starting server");

            Ok(cx.undefined().upcast())
        }

        method sync(mut cx) {
            let mut this = cx.this();
            //let js_sync_param = cx.argument::<JsObject>(0)?;
            //let js_forked = js_sync_param.get(&mut cx, "forked")?
            //    .downcast::<JsBoolean>()
            //    .or_throw(&mut cx)?;
            //let js_sync_infos = js_sync_param.get(&mut cx, "sync_infos")
            //    .downcast::<JsArray>()
            //    .or_throw(&mut cx)?;
            let forked = cx.argument::<JsBoolean>(0)?.value();
            let js_sync_infos_vec = cx.argument::<JsArray>(1)?.to_vec(&mut cx)?;
            let mut sync_infos: Vec<SyncInfo> = vec![];
            for i in 0..js_sync_infos_vec.len() {
                let js_sync_info = js_sync_infos_vec[i as usize]
                    .downcast::<JsObject>()
                    .or_throw(&mut cx)?;
                // extract transaction_info
                let js_transaction_info = js_sync_info.get(&mut cx, "transaction_info")?
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
                let js_header_info = js_sync_info.get(&mut cx, "header_info")?
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
                let js_deposition_requests = js_sync_info.get(&mut cx, "deposition_requests")?
                    .downcast::<JsArray>()
                    .or_throw(&mut cx)?
                    .to_vec(&mut cx)?;
                let mut deposition_requests: Vec<DepositionRequest> = vec![];
                for j in 0..js_deposition_requests.len() {
                    let js_deposition_request = js_deposition_requests[j as usize]
                        .downcast::<JsObject>()
                        .or_throw(&mut cx)?;
                    let js_pubkey_hash = js_deposition_request.get(&mut cx, "pubkey_hash")?
                        .downcast::<JsArrayBuffer>()
                        .or_throw(&mut cx)?;
                    let pubkey_hash = cx.borrow(&js_pubkey_hash, |data| {
                        let mut pubkey_hash = [0u8;20];
                        pubkey_hash[..].copy_from_slice(data.as_slice::<u8>());
                        pubkey_hash
                    });
                    let account_id = js_deposition_request.get(&mut cx, "account_id")?
                        .downcast::<JsNumber>()
                        .or_throw(&mut cx)?
                        .value() as u32;
                    let js_token_id = js_deposition_request.get(&mut cx, "token_id")?
                        .downcast::<JsArrayBuffer>()
                        .or_throw(&mut cx)?;
                    let token_id = cx.borrow(&js_token_id, |data| {
                        let mut token_id = [0u8;32];
                        token_id[..].copy_from_slice(data.as_slice::<u8>());
                        token_id
                    });
                    let value = js_deposition_request.get(&mut cx, "value")?
                        .downcast::<JsNumber>()
                        .or_throw(&mut cx)?
                        //TODO: update the value field type
                        .value() as u128;
                    let deposition_request = DepositionRequest {
                        pubkey_hash: pubkey_hash,
                        account_id: account_id,
                        token_id: token_id,
                        value: value
                    };
                    deposition_requests.push(deposition_request)
                }
                let sync_info = SyncInfo {
                    transaction_info: transaction_info,
                    header_info: header_info,
                    deposition_requests: deposition_requests
                };
                sync_infos.push(sync_info);
            }
            let sync_param = SyncParam {
                sync_infos: sync_infos,
                forked: forked
            };
            cx.borrow_mut(&mut this, |data| {
                let mut chain = &data.chain;
                chain.sync(sync_param).expect("Syncing chain");
            });
            Ok(cx.undefined().upcast())
        }
    }
}
