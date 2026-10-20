use super::*;
use axum::{
    extract::{ConnectInfo, Request},
    response::IntoResponse,
    Json, Router,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[tokio::test]
async fn rpc_reuses_connections_and_keeps_broadcast_outside_read_only_allowlist() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let (send, mut requests) = tokio::sync::mpsc::unbounded_channel();
    let router = Router::new().fallback(
        move |ConnectInfo(peer): ConnectInfo<std::net::SocketAddr>, Json(body): Json<Value>| {
            let send = send.clone();
            async move {
                send.send((peer, body)).unwrap();
                Json(json!({"jsonrpc":"2.0", "id":1, "result":"accepted"}))
            }
        },
    );
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            router.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .await
        .unwrap();
    });
    tokio::task::spawn_blocking(move || {
        let rpc = Rpc::new(&endpoint).unwrap();
        assert!(rpc.call("send_transaction", json!([])).is_err());
        assert_eq!(rpc.call("get_tip_header", json!([])).unwrap(), "accepted");
        assert_eq!(
            rpc.clone()
                .call("get_transaction", json!(["0x1234"]))
                .unwrap(),
            "accepted"
        );
        let tx = Transaction {
            version: "0x0".into(),
            cell_deps: vec![],
            header_deps: vec![],
            inputs: vec![],
            outputs: vec![],
            outputs_data: vec![],
            witnesses: vec![],
        };
        assert_eq!(broadcast(&endpoint, &tx).unwrap(), "accepted");
    })
    .await
    .unwrap();
    let (peer1, first) = requests.recv().await.unwrap();
    let (peer2, second) = requests.recv().await.unwrap();
    let (_, third) = requests.recv().await.unwrap();
    assert_eq!(
        peer1, peer2,
        "sequential RPCs must reuse the TCP connection"
    );
    assert_eq!(
        first,
        json!({"jsonrpc":"2.0", "id":1, "method":"get_tip_header", "params":[]})
    );
    assert_eq!(second["params"], json!(["0x1234"]));
    assert_eq!(third["method"], "send_transaction");
    assert_eq!(third["params"][1], "passthrough");
    assert!(
        requests.try_recv().is_err(),
        "denied writes must never reach the network"
    );
    server.abort();
}

#[tokio::test]
async fn rpc_rejects_http_and_json_errors_without_retrying_or_following_redirects() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let observed = calls.clone();
    let router = Router::new().fallback(move |request: Request| {
        observed.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        async move {
            match request.uri().path() {
                "/http" => axum::http::StatusCode::SERVICE_UNAVAILABLE.into_response(),
                "/redirect" => (
                    axum::http::StatusCode::TEMPORARY_REDIRECT,
                    [("location", "/valid")],
                )
                    .into_response(),
                "/json" => "not json".into_response(),
                "/id" => Json(json!({"jsonrpc":"2.0", "id":2, "result":null})).into_response(),
                "/error" => {
                    Json(json!({"jsonrpc":"2.0", "id":1, "error":{"code":-1}})).into_response()
                }
                "/missing" => Json(json!({"jsonrpc":"2.0", "id":1})).into_response(),
                _ => Json(json!({"jsonrpc":"2.0", "id":1, "result":null})).into_response(),
            }
        }
    });
    let server = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    tokio::task::spawn_blocking(move || {
        for path in ["http", "redirect", "json", "id", "error", "missing"] {
            assert!(
                Rpc::new(&format!("{endpoint}/{path}"))
                    .unwrap()
                    .call("get_tip_header", json!([]))
                    .is_err(),
                "{path}"
            );
        }
        assert!(Rpc::new(&endpoint)
            .unwrap()
            .call("get_tip_header", json!([]))
            .unwrap()
            .is_null());
    })
    .await
    .unwrap();
    assert_eq!(calls.load(std::sync::atomic::Ordering::Relaxed), 7);
    server.abort();
}

#[tokio::test]
async fn rpc_deadline_cancels_a_continuously_streaming_response() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        loop {
            request.push(stream.read_u8().await.unwrap());
            if request.ends_with(b"\r\n\r\n") {
                break;
            }
        }
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 10000\r\n\r\n")
            .await
            .unwrap();
        loop {
            if stream.write_all(b" ").await.is_err() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    });
    tokio::task::spawn_blocking(move || {
        let start = Instant::now();
        let result = with_deadline(start + Duration::from_millis(250), || {
            Rpc::new(&endpoint)
                .unwrap()
                .call("get_tip_header", json!([]))
        });
        assert!(result.is_err());
        assert!(start.elapsed() < Duration::from_secs(2));
        assert!(DEADLINE.get().is_none());
        let result = with_deadline(Instant::now() - Duration::from_secs(1), || {
            request(&endpoint, "get_tip_header", json!([]))
        });
        assert!(format!("{:#}", result.unwrap_err()).contains("deadline exceeded"));
    })
    .await
    .unwrap();
    tokio::time::timeout(Duration::from_secs(2), server)
        .await
        .unwrap()
        .unwrap();
}
