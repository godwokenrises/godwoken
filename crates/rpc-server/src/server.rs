// Taken and adapted from https://github.com/smol-rs/smol/blob/ad0839e1b3700dd33abb9bf23c1efd3c83b5bb2d/examples/hyper-server.rs
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Error, Result};
use gw_utils::liveness::Liveness;
use hyper::service::{make_service_fn, service_fn};
use hyper::{body::HttpBody, server::conn::AddrIncoming, Body, Method, Request, Response, Server};
use tokio::net::TcpListener;

use jsonrpc_v2::{RequestKind, ResponseObjects, Router, Server as JsonrpcServer};
use tokio::sync::{broadcast, mpsc};

use crate::registry::Registry;

pub async fn start_jsonrpc_server(
    listen_addr: SocketAddr,
    registry: Registry,
    liveness: Arc<Liveness>,
    _shutdown_send: mpsc::Sender<()>,
    mut sub_shutdown: broadcast::Receiver<()>,
) -> Result<()> {
    let rpc_server = registry.build_rpc_server()?;

    let listener = TcpListener::bind(listen_addr).await?;

    // Format the full address.
    let url = format!("http://{}", listener.local_addr()?);
    log::info!("JSONRPC server listening on {}", url);

    let mut incoming = AddrIncoming::from_listener(listener)?;
    incoming.set_keepalive(Some(Duration::from_secs(10)));

    // Start a hyper server.
    let server = Server::builder(incoming)
        .tcp_nodelay(true)
        .serve(make_service_fn(move |_| {
            let rpc_server = Arc::clone(&rpc_server);
            let liveness = liveness.clone();
            async move {
                Ok::<_, Error>(service_fn(move |req| {
                    serve(Arc::clone(&rpc_server), liveness.clone(), req)
                }))
            }
        }));
    let graceful = server.with_graceful_shutdown(async {
        let _ = sub_shutdown.recv().await;
        log::info!("rpc server exited successfully");
    });
    graceful.await?;

    Ok(())
}

// Serves a request and returns a response.
async fn serve<R: Router + 'static>(
    rpc: Arc<JsonrpcServer<R>>,
    liveness: Arc<Liveness>,
    req: Request<Body>,
) -> Result<Response<Body>> {
    if (req.method() == Method::GET || req.method() == Method::HEAD) && req.uri().path() == "/livez"
    {
        return hyper::Response::builder()
            .status(if liveness.is_live() {
                hyper::StatusCode::OK
            } else {
                hyper::StatusCode::SERVICE_UNAVAILABLE
            })
            .body(Body::empty())
            .map_err(anyhow::Error::new);
    }

    if (req.method() == Method::GET || req.method() == Method::HEAD)
        && req.uri().path() == "/metrics"
    {
        let mut buf = Vec::new();
        // XXX: HEAD response won't have content-length header.
        if req.method() != Method::HEAD {
            buf.reserve(1024);
            prometheus_client::encoding::text::encode(
                &mut buf,
                &*gw_metrics::REGISTRY.read().unwrap(),
            )?;
        }
        return hyper::Response::builder()
            .status(hyper::StatusCode::OK)
            .header(
                hyper::header::CONTENT_TYPE,
                "application/openmetrics-text; version=1.0.0; charset=utf-8",
            )
            .body(buf.into())
            .map_err(anyhow::Error::new);
    }

    if req.method() == Method::OPTIONS {
        return hyper::Response::builder()
            .status(hyper::StatusCode::NO_CONTENT)
            .header("Access-Control-Allow-Origin", "*")
            .header("Access-Control-Allow-Methods", "*")
            .header("Access-Control-Allow-Headers", "*")
            .body(Body::empty())
            .map_err(|e| anyhow::anyhow!("JSONRPC Preflight Request error: {:?}", e));
    }
    // Handler here is adapted from https://github.com/kardeiz/jsonrpc-v2/blob/1acf0b911c698413950d0b101ec4255cabd0d4ec/src/lib.rs#L1302
    let mut buf = if let Some(content_length) = req
        .headers()
        .get(hyper::header::CONTENT_LENGTH)
        .and_then(|x| x.to_str().ok())
        .and_then(|x| x.parse().ok())
    {
        bytes_v10::BytesMut::with_capacity(content_length)
    } else {
        bytes_v10::BytesMut::default()
    };

    let mut body = req.into_body();

    while let Some(chunk) = body.data().await {
        buf.extend(chunk?);
    }

    match rpc.handle(RequestKind::Bytes(buf.freeze())).await {
        ResponseObjects::Empty => hyper::Response::builder()
            .status(hyper::StatusCode::NO_CONTENT)
            .body(hyper::Body::from(Vec::<u8>::new()))
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>),
        json => serde_json::to_vec(&json)
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)
            .and_then(|json| {
                hyper::Response::builder()
                    .status(hyper::StatusCode::OK)
                    .header("Content-Type", "application/json")
                    .header("Access-Control-Allow-Origin", "*")
                    .header("Access-Control-Allow-Methods", "*")
                    .header("Access-Control-Allow-Headers", "*")
                    .body(hyper::Body::from(json))
                    .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)
            }),
    }
    .map_err(|e| anyhow::anyhow!("JSONRPC Request error: {:?}", e))
}
