use std::{net::SocketAddr, sync::Arc, time::Duration};

use anyhow::Result;
use axum::{
    extract::State,
    http::{header, HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Extension, Router,
};
use bytes::Bytes;
use gw_telemetry::{
    trace::http::HeaderExtractor,
    traits::{TelemetryContextNewSpan, TelemetryContextRemote},
};
use gw_utils::liveness::Liveness;
use hyper::server::conn::AddrIncoming;
use jsonrpc_core::MetaIoHandler;
use jsonrpc_utils::{axum_utils::handle_jsonrpc, pub_sub::Session};
use tokio::{
    net::TcpListener,
    sync::{broadcast, mpsc},
};
use tracing::Instrument;

pub async fn start_jsonrpc_server(
    listen_addr: SocketAddr,
    handler: Arc<MetaIoHandler<Option<Session>>>,
    liveness: Arc<Liveness>,
    _shutdown_send: mpsc::Sender<()>,
    mut sub_shutdown: broadcast::Receiver<()>,
) -> Result<()> {
    let listener = TcpListener::bind(listen_addr).await?;

    // Format the full address.
    let url = format!("http://{}", listener.local_addr()?);
    log::info!("JSONRPC server listening on {}", url);

    let mut incoming = AddrIncoming::from_listener(listener)?;
    incoming.set_keepalive(Some(Duration::from_secs(10)));
    incoming.set_nodelay(true);

    let app = Router::new()
        .route("/livez", get(serve_liveness))
        .with_state(liveness)
        .route("/metrics", get(serve_metrics))
        .route("/", post(handle_jsonrpc_with_tracing))
        .route("/*path", post(handle_jsonrpc_with_tracing))
        .with_state(handler);

    let server = axum::Server::builder(incoming).serve(app.into_make_service());
    let graceful = server.with_graceful_shutdown(async {
        let _ = sub_shutdown.recv().await;
        log::info!("rpc server exited successfully");
    });
    graceful.await?;

    Ok(())
}

async fn handle_jsonrpc_with_tracing(
    State(handler): State<Arc<MetaIoHandler<Option<Session>>>>,
    headers: HeaderMap,
    req_body: Bytes,
) -> impl IntoResponse {
    let remote_ctx = gw_telemetry::extract_context(&HeaderExtractor(&headers));
    let otel_ctx = gw_telemetry::current_context().with_remote_context(&remote_ctx);
    let serve_span = otel_ctx.new_span(tracing::info_span!("rpc.serve"));
    handle_jsonrpc(Extension(handler), req_body)
        .instrument(serve_span)
        .await
}

async fn serve_liveness(l: State<Arc<Liveness>>) -> impl IntoResponse {
    if l.is_live() {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    }
}

async fn serve_metrics() -> Result<impl IntoResponse, StatusCode> {
    let mut buf = Vec::new();
    gw_metrics::scrape(&mut buf).map_err(|_e| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok((
        [(
            header::CONTENT_TYPE,
            "application/openmetrics-text; version=1.0.0; charset=utf-8",
        )],
        buf,
    ))
}
