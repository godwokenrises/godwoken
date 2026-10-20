use super::*;
use clap::Parser;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

struct TestServer {
    address: std::net::SocketAddr,
    task: tokio::task::JoinHandle<Result<()>>,
    _web: tempfile::TempDir,
}
impl Drop for TestServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl TestServer {
    async fn start(timeout: u64) -> Self {
        #[derive(Parser)]
        struct Cli {
            #[command(flatten)]
            options: Options,
        }
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let web = tempfile::tempdir().unwrap();
        fs::write(web.path().join("index.html"), "Claim test").unwrap();
        let mut options = Cli::parse_from([
            "test",
            "--tables",
            ".",
            "--plan",
            "plan.json",
            "--web-dir",
            ".",
            "--dependencies",
            "dependencies.json",
        ])
        .options;
        options.public_origin = format!("http://{address}");
        options.body_timeout_secs = timeout;
        let app = Arc::new(App {
            options,
            rpc: Rpc::new("http://127.0.0.1:1").unwrap(),
            indexer: Rpc::new("http://127.0.0.1:1").unwrap(),
            assets: vec![],
            deps: vec![],
            deployment: None,
            entitlements: BTreeMap::new(),
            explanations: BTreeMap::new(),
            web: web.path().to_owned(),
            plan: serde_json::from_value(json!({
                "version":3, "deployment":"", "snapshot_root":"", "manifest":"test",
                "policy":"", "sources":[], "reserves":[], "claims":[], "limits":[],
                "state_root":"", "input_sha256":{}
            }))
            .unwrap(),
        });
        let portal = Arc::new(Portal {
            app,
            versions: BTreeMap::new(),
            jobs: Arc::new(Semaphore::new(MAX_JOBS)),
        });
        let router = Router::new()
            .fallback(handle)
            .with_state(portal)
            .layer(axum::middleware::map_response(secure_response));
        Self {
            address,
            task: tokio::spawn(serve(listener, router)),
            _web: web,
        }
    }

    async fn get(&self) {
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(1))
            .build()
            .unwrap();
        let response = client
            .get(format!("http://{}/api/v1/config", self.address))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["cache-control"], "no-store");
        assert_eq!(
            response.json::<Value>().await.unwrap()["plan_manifest"],
            "test"
        );
    }

    fn headers(&self, length: usize) -> String {
        format!("POST /api/v1/prepare HTTP/1.1\r\nHost: {0}\r\nOrigin: http://{0}\r\nContent-Type: application/json\r\nContent-Length: {length}\r\n\r\n", self.address)
    }
}

async fn closed_response(stream: &mut tokio::net::tcp::OwnedReadHalf, expected: u16) {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    tokio::time::timeout(Duration::from_secs(20), reader.read_line(&mut line))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        line.split_whitespace()
            .nth(1)
            .unwrap()
            .parse::<u16>()
            .unwrap(),
        expected
    );
    let mut rest = Vec::new();
    let result = tokio::time::timeout(Duration::from_secs(1), reader.read_to_end(&mut rest))
        .await
        .expect("server must close the socket while client is still sending");
    if let Err(e) = result {
        assert!(
            matches!(
                e.kind(),
                std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::ConnectionAborted
            ),
            "{e}"
        );
    }
    assert!(String::from_utf8_lossy(&rest).contains("connection: close"));
}

#[tokio::test]
async fn continuous_drip_after_408_closes_connections_and_keeps_queries_available() {
    // Both tiny_http's former small-body prebuffer path and its large-body reader.
    for (timeout, length) in [(1, 512), (1, MAX_BODY), (15, MAX_BODY)] {
        let server = TestServer::start(timeout).await;
        let mut readers = Vec::new();
        let mut writers = Vec::new();
        for _ in 0..4 {
            let mut stream = tokio::net::TcpStream::connect(server.address)
                .await
                .unwrap();
            stream
                .write_all(server.headers(length).as_bytes())
                .await
                .unwrap();
            let (reader, mut writer) = stream.into_split();
            readers.push(reader);
            writers.push(tokio::spawn(async move {
                loop {
                    if writer.write_all(b" ").await.is_err() {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            }));
        }
        server.get().await;
        for reader in &mut readers {
            closed_response(reader, 408).await;
        }
        // Keep writer tasks running after 408; only the server can end these connections.
        server.get().await;
        for writer in writers {
            tokio::time::timeout(Duration::from_secs(2), writer)
                .await
                .unwrap()
                .unwrap();
        }
    }
}

#[tokio::test]
async fn rejected_unread_bodies_close_without_draining() {
    let server = TestServer::start(1).await;
    let bad_origin = server.headers(MAX_BODY).replace(
        &format!("Origin: http://{}", server.address),
        "Origin: https://untrusted.invalid",
    );
    let missing_length = server.headers(1).replace("Content-Length: 1\r\n", "");
    let missing_v0 = server.headers(MAX_BODY).replace("/api/v1/", "/api/v0/");
    for (headers, status) in [
        (server.headers(MAX_BODY + 1), 413),
        (bad_origin, 400),
        (missing_length, 411),
        (missing_v0, 400),
    ] {
        let mut stream = tokio::net::TcpStream::connect(server.address)
            .await
            .unwrap();
        stream.write_all(headers.as_bytes()).await.unwrap();
        let (mut reader, _writer) = stream.into_split();
        closed_response(&mut reader, status).await;
        server.get().await;
    }
}

#[tokio::test]
async fn idle_body_and_slow_headers_expire() {
    let server = TestServer::start(1).await;
    let mut stream = tokio::net::TcpStream::connect(server.address)
        .await
        .unwrap();
    stream
        .write_all(server.headers(MAX_BODY).as_bytes())
        .await
        .unwrap();
    let (mut reader, _writer) = stream.into_split();
    closed_response(&mut reader, 408).await;

    let mut stream = tokio::net::TcpStream::connect(server.address)
        .await
        .unwrap();
    stream
        .write_all(b"POST /api/v1/prepare HTTP/1.1\r\nHost: ")
        .await
        .unwrap();
    let (mut reader, mut writer) = stream.into_split();
    let drip = tokio::spawn(async move {
        loop {
            if writer.write_all(b"x").await.is_err() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    });
    let mut bytes = Vec::new();
    let result = tokio::time::timeout(Duration::from_secs(5), reader.read_to_end(&mut bytes))
        .await
        .expect("header deadline must close a continuously slow header");
    assert!(result.is_ok() || result.unwrap_err().kind() == std::io::ErrorKind::ConnectionReset);
    tokio::time::timeout(Duration::from_secs(2), drip)
        .await
        .unwrap()
        .unwrap();
    server.get().await;
}

#[tokio::test]
async fn cancelled_or_timed_out_handler_keeps_its_slot_until_work_finishes() {
    for cancel in [true, false] {
        let slots = Arc::new(Semaphore::new(1));
        let (started, ready) = tokio::sync::oneshot::channel();
        let (finish, wait) = std::sync::mpsc::channel();
        let jobs = Arc::clone(&slots);
        let task = tokio::spawn(execute(jobs, Duration::from_millis(100), move || {
            started.send(()).unwrap();
            wait.recv().unwrap();
            Ok(())
        }));
        ready.await.unwrap();
        if cancel {
            task.abort();
        } else {
            assert_eq!(
                task.await.unwrap().unwrap_err().status(),
                StatusCode::GATEWAY_TIMEOUT
            );
        }
        assert_eq!(
            execute(Arc::clone(&slots), REQUEST_TIMEOUT, || Ok(()))
                .await
                .unwrap_err()
                .status(),
            StatusCode::SERVICE_UNAVAILABLE
        );
        finish.send(()).unwrap();
        let _permit = tokio::time::timeout(Duration::from_secs(1), slots.acquire())
            .await
            .unwrap()
            .unwrap();
    }
}
