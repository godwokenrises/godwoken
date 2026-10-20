#![cfg(feature = "e2e-tests")]
#[allow(dead_code)]
mod support;
use anyhow::{Context as _, Result};
use ckb_testtool::{
    ckb_types::{
        bytes::Bytes, core::TransactionView, packed as c, prelude::Entity as CEntity,
        prelude::IntoTransactionView,
    },
    context::Context,
};
use exit_withdraw_state_validator as contract;
use godwoken_withdraw::{
    db::Database,
    model::*,
    withdraw::{
        self,
        build::{Request, Skeleton},
        chain::Chain,
        Dependency, Deployment, Transaction,
    },
};
use gw_types::{packed, prelude::*};
use secp256k1::{Message, PublicKey, Secp256k1, SecretKey};
use serde_json::{json, Value};
use sha3::{Digest, Keccak256};
use std::{
    collections::BTreeMap,
    io::{Read, Write},
    net::TcpListener,
    path::Path,
    process::Command,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};
const CAP: u64 = 100_000_000;
fn key(n: u8) -> SecretKey {
    let mut k = [0; 32];
    k[31] = n;
    SecretKey::from_slice(&k).unwrap()
}
fn eth(n: u8) -> [u8; 20] {
    let pk = PublicKey::from_secret_key(&Secp256k1::new(), &key(n)).serialize_uncompressed();
    Keccak256::digest(&pk[1..])[12..].try_into().unwrap()
}
fn multisig(n: u8) -> Vec<u8> {
    let pk = PublicKey::from_secret_key(&Secp256k1::new(), &key(n)).serialize();
    [&[0, 0, 1, 1][..], &hash(&pk)[..20]].concat()
}
fn script(code: [u8; 32], args: Vec<u8>) -> Script {
    Script {
        code_hash: hx(code),
        hash_type: "type".into(),
        args: hx(args),
    }
}
fn omni_signature(signature: &[u8]) -> Vec<u8> {
    let total = 20 + signature.len();
    let mut b = Vec::new();
    for n in [total, 16, total, total, signature.len()] {
        b.extend_from_slice(&(n as u32).to_le_bytes());
    }
    b.extend_from_slice(signature);
    b
}
fn base_tx(outputs: Vec<CellOutput>, data: Vec<String>) -> Transaction {
    Transaction {
        version: "0x0".into(),
        cell_deps: vec![],
        header_deps: vec![],
        inputs: vec![],
        outputs,
        outputs_data: data,
        witnesses: vec![],
    }
}
fn point(tx: &Transaction, index: usize) -> OutPoint {
    OutPoint {
        tx_hash: tx.hash().unwrap(),
        index: format!("0x{index:x}"),
    }
}
#[derive(Clone, Default)]
struct Node {
    txs: BTreeMap<String, Transaction>,
    live: BTreeMap<OutPoint, Cell>,
    orphan: bool,
    unconfirmed: bool,
}
impl Node {
    fn insert(&mut self, tx: Transaction) {
        let hash = tx.hash().unwrap();
        for p in tx.input_points().unwrap() {
            self.live.remove(&p);
        }
        for i in 0..tx.outputs.len() {
            let p = point(&tx, i);
            self.live.insert(p.clone(), tx.cell(&p).unwrap());
        }
        self.txs.insert(hash, tx);
    }
    fn vm(&self, tx: &Transaction) -> Result<u64> {
        let mut ctx = Context::default();
        for p in tx.input_points()?.iter().chain(
            tx.cell_deps
                .iter()
                .map(|d| serde_json::from_value::<OutPoint>(d["out_point"].clone()).unwrap())
                .collect::<Vec<_>>()
                .iter(),
        ) {
            let cell = self.live.get(p).context("VM missing live cell")?;
            ctx.cells.insert(
                c::OutPoint::from_slice(withdraw::outpoint(p)?.as_slice())?,
                (
                    c::CellOutput::from_slice(cell.output.packed()?.as_slice())?,
                    Bytes::from(bytes(&cell.output_data)?),
                ),
            );
        }
        let packed = tx.packed()?;
        let tx: TransactionView = c::Transaction::from_slice(packed.as_slice())?.into_view();
        ctx.verify_tx(&tx, 70_000_000)
            .map_err(|e| anyhow::anyhow!("Full VM: {e:?}"))
    }
}
impl Chain for Node {
    fn call(&self, method: &str, params: Value) -> Result<Value> {
        let block = hx([42; 32]);
        let header = json!({"hash":block,"number":"0x64"});
        Ok(match method {
            "get_tip_header" => json!({"hash":hx([43;32]),"number":"0x80"}),
            "get_indexer_tip" => json!({"block_hash":hx([43;32]),"block_number":"0x80"}),
            "get_header" => header,
            "get_header_by_number" => {
                if params[0] == "0x0" {
                    json!({"hash":godwoken_withdraw::claim::GENESIS,"number":"0x0"})
                } else if params[0] == "0x80" {
                    json!({"hash":hx([43;32]),"number":"0x80"})
                } else if self.orphan {
                    json!({"hash":hx([44;32]),"number":"0x64"})
                } else {
                    header
                }
            }
            "get_transaction" => {
                let tx = self
                    .txs
                    .get(params[0].as_str().unwrap())
                    .context("Pruned history")?;
                json!({"transaction":tx,"tx_status":{"status":if self.unconfirmed{"pending"}else{"committed"},"block_hash":block}})
            }
            "get_live_cell" => {
                let p: OutPoint = serde_json::from_value(params[0].clone())?;
                match self.live.get(&p) {
                    Some(c) => {
                        json!({"status":"live","cell":{"output":c.output,"data":{"content":c.output_data}}})
                    }
                    None => json!({"status":"dead"}),
                }
            }
            "get_transactions" => {
                let requested: Script = serde_json::from_value(params[0]["script"].clone())?;
                let objects = self.txs.iter().filter_map(|(hash, tx)| {
                    let cells = tx.outputs.iter().enumerate().filter(|(_,o)|o.lock==requested).map(|(i,_)|json!(["output",i])).collect::<Vec<_>>();
                    (!cells.is_empty()).then(||json!({"tx_hash":hash,"block_number":"0x64","tx_index":"0x0","cells":cells}))
                }).collect::<Vec<_>>();
                json!({"objects":objects,"last_cursor":"0x00"})
            }
            "get_cells" => {
                let requested: Script = serde_json::from_value(params[0]["script"].clone())?;
                if params.as_array().unwrap().len() > 3 {
                    json!({"objects":[],"last_cursor":"end"})
                } else {
                    let mut matches = vec![];
                    for c in self.live.values() {
                        let actual = if params[0]["script_type"] == "lock" {
                            Some(&c.output.lock)
                        } else {
                            c.output.type_script.as_ref()
                        };
                        if let Some(a) = actual {
                            if a.code_hash == requested.code_hash
                                && a.hash_type == requested.hash_type
                                && (if params[0]["script_search_mode"] == "prefix" {
                                    a.args.starts_with(&requested.args)
                                } else {
                                    a.args == requested.args
                                })
                            {
                                let mut c = c.clone();
                                c.block_number = "0x64".into();
                                c.tx_index = "0x0".into();
                                matches.push(c);
                            }
                        }
                    }
                    json!({"objects":matches,"last_cursor":"end"})
                }
            }
            "estimate_cycles" => {
                json!({"cycles":format!("0x{:x}",self.vm(&serde_json::from_value(params[0].clone())?)?)})
            }
            _ => anyhow::bail!("Forbidden/unexpected method {method}"),
        })
    }
}
struct Setup {
    base: contract::Profile,
    d: Deployment,
    b: Bundle,
    node: Node,
}
fn sign_group(tx: &mut Transaction, indices: &[usize], secret: u8, omni: bool) {
    sign_group_with_mode(tx, indices, secret, omni, omni)
}
fn sign_group_with_mode(
    tx: &mut Transaction,
    indices: &[usize],
    secret: u8,
    omni: bool,
    ethereum: bool,
) {
    let mut hasher = gw_hash::blake2b::new_blake2b();
    hasher.update(&hash32(&tx.hash().unwrap()).unwrap());
    for &i in indices {
        let mut w = bytes(&tx.witnesses[i]).unwrap();
        if omni && i == indices[0] {
            let args = packed::WitnessArgs::from_slice(&w).unwrap();
            let len = args.lock().to_opt().unwrap().raw_data().len();
            w = args
                .as_builder()
                .lock(Some(gw_types::bytes::Bytes::from(vec![0; len])).pack())
                .build()
                .as_slice()
                .to_vec();
        }
        hasher.update(&(w.len() as u64).to_le_bytes());
        hasher.update(&w);
    }
    for w in &tx.witnesses[tx.inputs.len()..] {
        let b = bytes(w).unwrap();
        hasher.update(&(b.len() as u64).to_le_bytes());
        hasher.update(&b);
    }
    let mut digest = [0; 32];
    hasher.finalize(&mut digest);
    if ethereum {
        digest =
            Keccak256::digest([b"\x19Ethereum Signed Message:\n32".as_slice(), &digest].concat())
                .into();
    }
    let sig = Secp256k1::new().sign_ecdsa_recoverable(&Message::from_digest(digest), &key(secret));
    let (recid, compact) = sig.serialize_compact();
    let signature = [compact.as_slice(), &[i32::from(recid) as u8]].concat();
    let w = packed::WitnessArgs::from_slice(&bytes(&tx.witnesses[indices[0]]).unwrap()).unwrap();
    let lock = if omni {
        omni_signature(&signature)
    } else {
        [multisig(secret).as_slice(), &signature].concat()
    };
    tx.witnesses[indices[0]] = hx(w
        .as_builder()
        .lock(Some(gw_types::bytes::Bytes::from(lock)).pack())
        .build()
        .as_slice());
}
struct Server {
    url: String,
    node: Arc<Mutex<Node>>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Server {
    fn new(node: Node) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        listener.set_nonblocking(true).unwrap();
        let node = Arc::new(Mutex::new(node));
        let stop = Arc::new(AtomicBool::new(false));
        let n = node.clone();
        let st = stop.clone();
        let thread = std::thread::spawn(move || {
            while !st.load(Ordering::Relaxed) {
                let (mut stream, _) = match listener.accept() {
                    Ok(s) => s,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(std::time::Duration::from_millis(1));
                        continue;
                    }
                    Err(e) => panic!("{e}"),
                };
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(std::time::Duration::from_secs(10)))
                    .unwrap();
                let mut data = Vec::new();
                let mut buf = [0; 8192];
                let header_end = loop {
                    let count = stream.read(&mut buf).unwrap();
                    assert!(count > 0);
                    data.extend_from_slice(&buf[..count]);
                    if let Some(i) = data.windows(4).position(|x| x == b"\r\n\r\n") {
                        break i + 4;
                    }
                };
                let length = String::from_utf8_lossy(&data[..header_end])
                    .lines()
                    .find_map(|l| {
                        l.to_ascii_lowercase()
                            .strip_prefix("content-length:")
                            .map(|s| s.trim().parse::<usize>().unwrap())
                    })
                    .unwrap();
                while data.len() < header_end + length {
                    let count = stream.read(&mut buf).unwrap();
                    assert!(count > 0);
                    data.extend_from_slice(&buf[..count]);
                }
                let req: Value =
                    serde_json::from_slice(&data[header_end..header_end + length]).unwrap();
                let result = {
                    let mut node = n.lock().unwrap();
                    if req["method"] == "send_transaction" {
                        (|| -> Result<Value> {
                            let tx: Transaction = serde_json::from_value(req["params"][0].clone())?;
                            node.vm(&tx)?;
                            let hash = tx.hash()?;
                            node.insert(tx);
                            Ok(json!(hash))
                        })()
                    } else {
                        node.call(req["method"].as_str().unwrap(), req["params"].clone())
                    }
                };
                let response=match result{Ok(v)=>json!({"jsonrpc":"2.0","id":req["id"],"result":v}),Err(e)=>json!({"jsonrpc":"2.0","id":req["id"],"error":{"code":-1,"message":format!("{e:#}")}})}.to_string();
                write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",response.len(),response).unwrap();
            }
        });
        Self {
            url,
            node,
            stop,
            thread: Some(thread),
        }
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = self.thread.take().unwrap().join();
    }
}

#[path = "support/settlement_e2e.rs"]
mod settlement_e2e;

#[path = "support/claim_e2e.rs"]
mod claim_e2e;

#[path = "support/refund_shutdown.rs"]
mod refund_shutdown;

#[path = "support/upgrade_e2e.rs"]
mod upgrade_e2e;
