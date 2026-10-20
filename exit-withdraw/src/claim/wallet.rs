//! Ordinary mainnet OmniLock funds: ownership, not refund provenance, authorizes a spend.
use super::*;
use crate::{
    network::{broadcast, Rpc},
    withdraw::chain::{self, History},
};
use axum::{
    extract::{Request, State},
    http::{header, Method, StatusCode},
    response::{IntoResponse, Response},
    Router,
};
use clap::Args;
use gw_types::U256;
use serde_json::Value;
use std::{fs, path::PathBuf, sync::Arc, time::Duration};
use tokio::sync::Semaphore;

pub const MAX_FEE: u64 = 1_000_000; // 0.01 CKB, checked independently of the fee rate.

#[derive(Args, Clone)]
pub struct Options {
    #[arg(long, default_value = "127.0.0.1:8787")]
    pub listen: String,
    #[arg(long, default_value = "http://127.0.0.1:8787")]
    pub public_origin: String,
    #[arg(long, default_value = "https://mainnet.ckb.dev/rpc")]
    pub ckb_rpc: String,
    #[arg(long)]
    pub indexer_rpc: Option<String>,
    #[arg(long)]
    pub web_dir: PathBuf,
    #[arg(long, default_value_t = 1000)]
    pub fee_rate: u64,
    /// Include committed cells after this many blocks. No refund-source restriction.
    #[arg(long, default_value_t = 1)]
    pub confirmations: u64,
    /// Relay only transactions the owner reviewed and signed.
    #[arg(long)]
    pub enable_broadcast: bool,
    #[arg(long)]
    pub dependencies: Option<PathBuf>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Submit {
    request: BuildRequest,
    transaction_hash: String,
    signing_message: String,
    signature: String,
}

struct Wallet {
    options: Options,
    rpc: Rpc,
    indexer: Rpc,
    deps: Vec<Dependency>,
    web: PathBuf,
    jobs: Arc<Semaphore>,
}

fn asset_id(hash: &str) -> Result<u32> {
    let id = u32::from_be_bytes(hash32(hash)?[..4].try_into()?);
    ensure!(
        id > 1,
        "Token identifier collision; inspect this token on Explorer"
    );
    Ok(id)
}

/// The catalogue supplies display labels only. Any standard sUDT may be transferred.
pub fn assets(cells: &[Cell]) -> Result<Vec<Asset>> {
    let labels: Vec<Asset> =
        serde_json::from_str(include_str!("../../claim-web/mainnet-assets.json"))?;
    let mut found = BTreeMap::new();
    found.insert(
        1,
        Asset {
            asset_id: 1,
            symbol: Some("CKB".into()),
            decimals: Some(8),
            l1_decimals: Some(8),
            type_hash: NATIVE.into(),
        },
    );
    for cell in cells {
        let Some(t) = &cell.output.type_script else {
            continue;
        };
        if t.code_hash != SUDT
            || t.hash_type != "type"
            || bytes(&t.args)?.len() != 32
            || bytes(&cell.output_data)?.len() != 16
        {
            continue;
        }
        let hash = t.hash()?;
        let id = asset_id(&hash)?;
        let label = labels.iter().find(|a| a.type_hash == hash);
        let asset = Asset {
            asset_id: id,
            type_hash: hash,
            symbol: label.and_then(|a| a.symbol.clone()),
            decimals: label.and_then(|a| a.l1_decimals),
            l1_decimals: label.and_then(|a| a.l1_decimals),
        };
        if let Some(old) = found.get(&id) {
            ensure!(
                old.type_hash == asset.type_hash,
                "Token identifier collision"
            );
        }
        found.insert(id, asset);
    }
    Ok(found.into_values().collect())
}

fn balances(cells: &[Cell], assets: &[Asset]) -> Result<Vec<Value>> {
    let mut totals = BTreeMap::<u32, U256>::new();
    for cell in cells {
        let n = totals.entry(1).or_default();
        *n = n
            .checked_add(quantity(&cell.output.capacity)?.into())
            .context("Capacity overflow")?;
        if let Some(t) = &cell.output.type_script {
            if let Some(a) = assets
                .iter()
                .find(|a| t.hash().ok().as_ref() == Some(&a.type_hash))
            {
                let n = totals.entry(a.asset_id).or_default();
                *n = n
                    .checked_add(U256::from_little_endian(&bytes(&cell.output_data)?))
                    .context("Token overflow")?;
            }
        }
    }
    Ok(totals
        .into_iter()
        .map(|(id, n)| json!({"asset_id":id,"raw":n.to_string()}))
        .collect())
}

impl Wallet {
    fn network(&self) -> Result<()> {
        ensure!(
            self.rpc.call("get_header_by_number", json!(["0x0"]))?["hash"] == GENESIS,
            "RPC must be CKB mainnet"
        );
        Ok(())
    }
    fn inventory(&self, eth: &str) -> Result<Value> {
        self.network()?;
        let eth = hx(eth_address(eth)?);
        let lock = owner(&eth)?;
        let address = address(&lock)?;
        let tip = self.rpc.call("get_tip_header", json!([]))?;
        let tip_n = quantity(tip["number"].as_str().context("Missing tip")?)?;
        let indexed = self.indexer.call("get_indexer_tip", json!([]))?;
        let index_n = quantity(
            indexed["block_number"]
                .as_str()
                .context("Indexer unavailable")?,
        )?;
        ensure!(
            index_n <= tip_n && tip_n - index_n <= 20,
            "Indexer is behind the mainnet node; refresh shortly"
        );
        ensure!(
            self.rpc
                .call("get_header_by_number", json!([indexed["block_number"]]))?["hash"]
                == indexed["block_hash"],
            "Indexer is on a different chain"
        );
        let all = self.indexer.cells(&lock, "lock", "exact")?;
        let catalogue = assets(&all)?;
        let mut spendable = Vec::new();
        let mut other = Vec::new();
        let mut confirming = 0;
        for cell in &all {
            if !supported(cell, &catalogue)? {
                other.push(cell.clone());
                continue;
            }
            if tip_n
                .checked_sub(quantity(&cell.block_number)?)
                .and_then(|n| n.checked_add(1))
                .unwrap_or(0)
                < self.options.confirmations
            {
                confirming += 1;
                continue;
            }
            spendable.push(cell.clone());
        }
        let transferable = balances(&spendable, &catalogue)?;
        let more = spendable.len() > MAX_INPUTS;
        spendable.truncate(MAX_INPUTS);
        Ok(
            json!({"address":eth,"omnilock_address":address,"assets":catalogue,"balances":balances(&all,&catalogue)?,"transferable":transferable,"available":balances(&spendable,&catalogue)?,"cells":spendable,"other_cells":other,"more_batches":more,"confirming_cells":confirming,"confirmations_required":self.options.confirmations,"observed_block":tip_n,"explorer_url":format!("https://explorer.nervos.org/address/{address}")}),
        )
    }
    fn transactions(&self, eth: &str, cursor: Option<&str>) -> Result<Value> {
        let lock = owner(eth)?;
        let mut params = vec![
            json!({"script":lock,"script_type":"lock","script_search_mode":"exact","group_by_transaction":true}),
            json!("desc"),
            json!("0x14"),
        ];
        if let Some(cursor) = cursor {
            ensure!(
                cursor.starts_with("0x") && cursor.len() <= 4096,
                "Invalid transaction cursor"
            );
            bytes(cursor)?;
            params.push(json!(cursor));
        }
        let page = self.indexer.call("get_transactions", json!(params))?;
        let objects = page["objects"]
            .as_array()
            .context("Invalid transaction history")?;
        Ok(
            json!({"transactions":objects,"last_cursor":page["last_cursor"],"more":objects.len()==20}),
        )
    }
    fn dependencies(&self) -> Result<Vec<Cell>> {
        let mut cells = Vec::new();
        let mut seen = BTreeSet::new();
        for dep in &self.deps {
            ensure!(
                dep.dep_type == "code" && seen.insert(dep.out_point.clone()),
                "Invalid dependency"
            );
            let c = chain::live(&self.rpc, &dep.out_point)?;
            ensure!(
                hx(hash(&bytes(&c.output_data)?)) == dep.data_hash,
                "Dependency binary hash changed"
            );
            cells.push(c);
        }
        ensure!(
            cells.iter().any(
                |c| c.output.type_script.as_ref().and_then(|s| s.hash().ok()) == Some(hx(c::OMNI))
            ),
            "Missing mainnet OmniLock dependency"
        );
        ensure!(
            cells
                .iter()
                .any(|c| hx(hash(&bytes(&c.output_data).unwrap_or_default()))
                    == "0x9799bee251b975b82c45a02154ce28cec89c5853ecc14d12b7b8cccfc19e0af4"),
            "Missing secp256k1 data"
        );
        Ok(cells)
    }
    fn prepare(&self, request: &BuildRequest) -> Result<Quote> {
        self.network()?;
        ensure!(
            !request.inputs.is_empty() && request.inputs.len() <= MAX_INPUTS,
            "Select between 1 and 64 cells"
        );
        let deps = self.dependencies()?;
        let mut history = History::new(&self.rpc, self.options.confirmations)?;
        let mut cells = Vec::new();
        for point in &request.inputs {
            let live = chain::live(&self.rpc, point)?;
            let committed = history.cell(point)?;
            ensure!(
                chain::same_cell(&live, &committed),
                "Input differs from confirmed chain history"
            );
            cells.push(live);
        }
        let catalogue = assets(&cells)?;
        let quote = construct(
            request,
            cells,
            &catalogue,
            &self.deps,
            self.options.fee_rate,
        )?;
        ensure!(
            quantity_decimal(&quote.fee_shannons)? <= MAX_FEE,
            "Network fee exceeds 0.01 CKB; select fewer cells"
        );
        withdraw::verify_script_dependencies(
            &quote.input_cells,
            &quote.transaction.outputs,
            &deps,
        )?;
        history.recheck()?;
        Ok(quote)
    }
    fn validate_submission(&self, request: &Submit) -> Result<(Quote, Transaction, u64)> {
        ensure!(
            self.options.enable_broadcast,
            "Transaction submission is disabled"
        );
        let quote = self.prepare(&request.request)?;
        ensure!(
            quote.transaction_hash == request.transaction_hash
                && quote.signing_message == request.signing_message,
            "Transaction changed; review and sign again"
        );
        let tx = assemble(&quote, &request.signature)?;
        let result = self.rpc.call("estimate_cycles", json!([tx]))?;
        let cycles = quantity(
            result["cycles"]
                .as_str()
                .context("Node cannot validate scripts")?,
        )?;
        ensure!(cycles <= MAX_CYCLES, "Transaction cycle limit exceeded");
        for cell in &quote.input_cells {
            self.rpc.verify_live(cell)?;
        }
        Ok((quote, tx, cycles))
    }
    fn submit(&self, request: Submit) -> Result<Value> {
        let (quote, tx, cycles) = match self.validate_submission(&request) {
            Ok(validated) => validated,
            Err(e) => return Ok(json!({"error":e.to_string(),"broadcast_attempted":false})),
        };
        let hash = broadcast(&self.rpc.endpoint, &tx)?;
        ensure!(
            hash.as_str() == Some(&quote.transaction_hash),
            "Check the reviewed transaction hash before retrying"
        );
        Ok(json!({"transaction_hash":hash,"status":"submitted","cycles":cycles}))
    }
    fn route(&self, method: &Method, uri: &str, body: &[u8]) -> Result<(Vec<u8>, &'static str)> {
        let (path, query) = uri.split_once('?').unwrap_or((uri, ""));
        let result = match (method, path) {
            (&Method::GET, "/api/wallet/config") => {
                json!({"network":"CKB mainnet","scope":"OmniLock wallet","assets":assets(&[])?,"dependencies":self.deps,"broadcast_enabled":self.options.enable_broadcast,"fee_rate":self.options.fee_rate,"max_fee_shannons":MAX_FEE.to_string()})
            }
            (&Method::GET, p) if p.starts_with("/api/wallet/account/") => {
                self.inventory(&p[20..])?
            }
            (&Method::GET, p) if p.starts_with("/api/wallet/transactions/") => {
                self.transactions(&p[25..], query.strip_prefix("cursor="))?
            }
            (&Method::GET, p) if p.starts_with("/api/wallet/transaction/") => {
                let hash = hx(hash32(&p[24..])?);
                let status = self.rpc.call("get_transaction", json!([hash]))?;
                json!({"transaction_hash":hash,"status":status["tx_status"]["status"],"reason":status["tx_status"]["reason"]})
            }
            (&Method::POST, "/api/wallet/prepare") => {
                serde_json::to_value(self.prepare(&serde_json::from_slice(body)?)?)?
            }
            (&Method::POST, "/api/wallet/submit") => self.submit(serde_json::from_slice(body)?)?,
            (&Method::GET, p) if !p.starts_with("/api/") => {
                let relative = if p == "/" {
                    "index.html"
                } else {
                    p.trim_start_matches('/')
                };
                let file = self.web.join(relative).canonicalize()?;
                ensure!(
                    file.starts_with(&self.web) && file.is_file(),
                    "Page not found"
                );
                let mime = match file.extension().and_then(|s| s.to_str()) {
                    Some("html") => "text/html; charset=utf-8",
                    Some("js") => "text/javascript; charset=utf-8",
                    Some("css") => "text/css; charset=utf-8",
                    Some("svg") => "image/svg+xml",
                    _ => "application/octet-stream",
                };
                return Ok((fs::read(file)?, mime));
            }
            _ => anyhow::bail!("Unknown wallet route"),
        };
        Ok((serde_json::to_vec(&result)?, "application/json"))
    }
}

fn quantity_decimal(s: &str) -> Result<u64> {
    s.parse().context("Invalid network fee")
}

async fn handle(State(wallet): State<Arc<Wallet>>, request: Request) -> Response {
    let method = request.method().clone();
    let uri = request
        .uri()
        .path_and_query()
        .map(|p| p.as_str())
        .unwrap_or("/")
        .to_owned();
    let host = wallet.options.public_origin.split_once("://").unwrap().1;
    if request
        .headers()
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        != Some(host)
    {
        return server::error(StatusCode::BAD_REQUEST, "Invalid request host");
    }
    if method == Method::POST
        && (request
            .headers()
            .get(header::ORIGIN)
            .and_then(|h| h.to_str().ok())
            != Some(&wallet.options.public_origin)
            || !request
                .headers()
                .get(header::CONTENT_TYPE)
                .and_then(|h| h.to_str().ok())
                .is_some_and(|s| s.starts_with("application/json")))
    {
        return server::error(StatusCode::BAD_REQUEST, "Use a same-origin JSON request");
    }
    let body = if method == Method::POST {
        match server::read_body(request, Duration::from_secs(15)).await {
            Ok(b) => b,
            Err(r) => return r,
        }
    } else {
        axum::body::Bytes::new()
    };
    match server::execute(
        Arc::clone(&wallet.jobs),
        Duration::from_secs(120),
        move || wallet.route(&method, &uri, &body),
    )
    .await
    {
        Ok((body, mime)) => ([(header::CONTENT_TYPE, mime)], body).into_response(),
        Err(r) => r,
    }
}

pub fn run(options: Options) -> Result<()> {
    ensure!(
        (1000..=10_000).contains(&options.fee_rate),
        "Use a fee rate between 1000 and 10000 shannons/KB"
    );
    ensure!(
        (1..=10000).contains(&options.confirmations),
        "Invalid confirmation depth"
    );
    ensure!(
        (options.public_origin.starts_with("http://")
            || options.public_origin.starts_with("https://"))
            && !options.public_origin.ends_with('/')
            && !options.public_origin.contains(['?', '#', '@']),
        "Use an exact public origin"
    );
    let deps: Vec<Dependency> = if let Some(path) = &options.dependencies {
        serde_json::from_slice(&fs::read(path)?)?
    } else {
        serde_json::from_str(include_str!("../../claim-web/mainnet-dependencies.json"))?
    };
    ensure!(
        !deps.is_empty() && deps.len() <= 8,
        "Use 1-8 code dependencies"
    );
    let web = options.web_dir.canonicalize()?;
    ensure!(
        web.join("index.html").is_file(),
        "Build the wallet UI first"
    );
    let app = Arc::new(Wallet {
        rpc: Rpc::new(&options.ckb_rpc)?,
        indexer: Rpc::new(options.indexer_rpc.as_deref().unwrap_or(&options.ckb_rpc))?,
        deps,
        web,
        jobs: Arc::new(Semaphore::new(4)),
        options,
    });
    eprintln!(
        "Mainnet OmniLock wallet: {} (broadcast enabled: {})",
        app.options.public_origin, app.options.enable_broadcast
    );
    let router = Router::new()
        .fallback(handle)
        .with_state(Arc::clone(&app))
        .layer(axum::middleware::map_response(server::secure_response));
    tokio::runtime::Builder::new_multi_thread().enable_all().build()?.block_on(async move {
        let listener = tokio::net::TcpListener::bind(&app.options.listen).await?;
        tokio::select! { result=server::serve(listener,router)=>result, result=tokio::signal::ctrl_c()=>{result?;Ok(())} }
    })
}
