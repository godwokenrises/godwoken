// Taken and adapted from https://github.com/smol-rs/smol/blob/ad0839e1b3700dd33abb9bf23c1efd3c83b5bb2d/examples/hyper-server.rs
use std::net::SocketAddr;
use std::net::{Shutdown, TcpListener, TcpStream};
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use anyhow::{Error, Result};
use hyper::service::{make_service_fn, service_fn};
use hyper::{body::HttpBody, Body, Method, Request, Response, Server};
use smol::{io, prelude::*, Async};

use jsonrpc_v2::{RequestKind, ResponseObjects, Router, Server as JsonrpcServer};

use crate::registry::Registry;

pub async fn start_jsonrpc_server(listen_addr: SocketAddr, registry: Registry) -> Result<()> {
    let rpc_server = registry.build_rpc_server()?;
    let listener = Async::<TcpListener>::bind(listen_addr)?;

    // Format the full address.
    let url = format!("http://{}", listener.get_ref().local_addr()?);
    println!("JSONRPC server listening on {}", url);

    // Start a hyper server.
    Server::builder(SmolListener::new(&listener))
        .executor(SmolExecutor)
        .serve(make_service_fn(move |_| {
            let rpc_server = Arc::clone(&rpc_server);
            async { Ok::<_, Error>(service_fn(move |req| serve(Arc::clone(&rpc_server), req))) }
        }))
        .await?;

    Ok(())
}

// Serves a request and returns a response.
async fn serve<R: Router + 'static>(
    rpc: Arc<JsonrpcServer<R>>,
    req: Request<Body>,
) -> Result<Response<Body>> {
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

// Spawns futures.
#[derive(Clone)]
struct SmolExecutor;

impl<F: Future + Send + 'static> hyper::rt::Executor<F> for SmolExecutor {
    fn execute(&self, fut: F) {
        smol::spawn(async { drop(fut.await) }).detach();
    }
}

// Listens for incoming connections.
struct SmolListener<'a> {
    incoming: Pin<Box<dyn Stream<Item = io::Result<Async<TcpStream>>> + Send + 'a>>,
}

impl<'a> SmolListener<'a> {
    fn new(listener: &'a Async<TcpListener>) -> Self {
        Self {
            incoming: Box::pin(listener.incoming()),
        }
    }
}

impl hyper::server::accept::Accept for SmolListener<'_> {
    type Conn = SmolStream;
    type Error = Error;

    fn poll_accept(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
    ) -> Poll<Option<Result<Self::Conn, Self::Error>>> {
        let stream = smol::ready!(self.incoming.as_mut().poll_next(cx)).unwrap()?;

        let stream = SmolStream::Plain(stream);

        Poll::Ready(Some(Ok(stream)))
    }
}

// A TCP or TCP+TLS connection.
enum SmolStream {
    // A plain TCP connection.
    Plain(Async<TcpStream>),
}

impl hyper::client::connect::Connection for SmolStream {
    fn connected(&self) -> hyper::client::connect::Connected {
        hyper::client::connect::Connected::new()
    }
}

impl tokio::io::AsyncRead for SmolStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        match &mut *self {
            SmolStream::Plain(s) => {
                return Pin::new(s)
                    .poll_read(cx, buf.initialize_unfilled())
                    .map_ok(|size| {
                        buf.advance(size);
                    });
            }
        }
    }
}

impl tokio::io::AsyncWrite for SmolStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        match &mut *self {
            SmolStream::Plain(s) => Pin::new(s).poll_write(cx, buf),
        }
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match &mut *self {
            SmolStream::Plain(s) => Pin::new(s).poll_flush(cx),
        }
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match &mut *self {
            SmolStream::Plain(s) => {
                s.get_ref().shutdown(Shutdown::Write)?;
                Poll::Ready(Ok(()))
            }
        }
    }
}
