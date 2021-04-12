use std::net::SocketAddr;
use std::net::{Shutdown, TcpListener, TcpStream};
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use anyhow::{anyhow, Result};
use ckb_types::prelude::Entity;
use faster_hex::hex_decode;
use gw_types::packed;
use hyper::service::{make_service_fn, service_fn};
use hyper::{body::HttpBody, Body, Request, Response};
use parking_lot::Mutex;
use smol::{io, prelude::*, Async};

use jsonrpc_v2::{
    Data, MapRouter, Params, RequestKind, ResponseObjects, Router, Server, Server as JsonrpcServer,
};
use log::debug;
type RPCServer = Arc<Server<MapRouter>>;

type MemPool = Arc<Mutex<gw_mem_pool::pool::MemPool>>;

pub struct Registry {
    mem_pool: MemPool,
}

impl Registry {
    pub fn build_rpc_server(self) -> Result<RPCServer> {
        let mut server = JsonrpcServer::new();

        server = server
            .with_data(Data(self.mem_pool.clone()))
            .with_method("execute_l2transaction", execute_l2transaction);

        Ok(server.finish())
    }
}

fn decode_hex_str(hex_str: String) -> Result<Vec<u8>> {
    if !hex_str.starts_with("0x") {
        return Err(anyhow!("Invalid hex string"));
    }
    let (_prefix, str) = hex_str.split_at(2);
    let mut dst = Vec::new();
    dst.resize(str.len() / 2, 0);
    hex_decode(str.as_bytes(), &mut dst);
    Ok(dst)
}

async fn execute_l2transaction(
    Params(params): Params<String>,
    mem_pool: Data<MemPool>,
) -> Result<usize> {
    let l2tx_bytes = decode_hex_str(params)?;
    let tx = packed::L2Transaction::from_slice(&l2tx_bytes)?;
    Ok(params.0 - params.1)
}
