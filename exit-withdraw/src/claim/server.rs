//! Same-origin, keyless claim API. The audit RPC allowlist stays read-only.
use super::*;
use crate::{
    network::Rpc,
    settlement::{
        adapter::{Profile, Protocol},
        plan::Plan,
    },
    withdraw::chain::{self, History},
};
use clap::Args;
use exit_withdraw_state_validator::settlement::records::*;
use serde_json::Value;
use sha2::Sha256;
use std::{
    fs,
    io::{self, Write},
    path::PathBuf,
    process::{Command, Stdio},
    sync::Arc,
    time::{Duration, Instant},
};
use tiny_http::{Header, Method, Request, Response, Server, StatusCode};

#[derive(Args, Clone)]
pub struct Options {
    #[arg(long, default_value = "127.0.0.1:8787")]
    pub listen: String,
    /// Maximum time spent reading one JSON request body. The socket idle timeout is shorter.
    #[arg(long, default_value_t = 15)]
    pub body_timeout_secs: u64,
    #[arg(long, default_value = "http://127.0.0.1:8787")]
    pub public_origin: String,
    #[arg(long, default_value = "https://mainnet.ckb.dev/rpc")]
    pub ckb_rpc: String,
    #[arg(long)]
    pub indexer_rpc: Option<String>,
    #[arg(long)]
    pub tables: PathBuf,
    #[arg(long)]
    pub plan: PathBuf,
    /// Published v1 release deployment; required to classify spendable cells.
    #[arg(long)]
    pub deployment: Option<PathBuf>,
    #[arg(long, requires = "v0_plan")]
    pub v0_tables: Option<PathBuf>,
    #[arg(long, requires = "v0_tables")]
    pub v0_plan: Option<PathBuf>,
    #[arg(long, requires = "v0_plan")]
    pub v0_deployment: Option<PathBuf>,
    #[arg(long)]
    pub web_dir: PathBuf,
    #[arg(long)]
    pub dependencies: PathBuf,
    #[arg(long, default_value_t = 1000)]
    pub fee_rate: u64,
    #[arg(long, default_value_t = 24)]
    pub confirmations: u64,
    /// Permit forwarding only user-signed, rebuilt and VM-validated L1 claims.
    #[arg(long)]
    pub enable_broadcast: bool,
    /// Immutable Ops-prepared control proposals, partitioned into v0/ and v1/ directories.
    #[arg(long)]
    pub control_proposals: Option<PathBuf>,
}
struct App {
    options: Options,
    rpc: Rpc,
    indexer: Rpc,
    assets: Vec<Asset>,
    deps: Vec<Dependency>,
    plan: Plan,
    deployment: Option<withdraw::Deployment>,
    entitlements: BTreeMap<String, BTreeMap<u32, gw_types::U256>>,
    explanations: BTreeMap<String, Vec<Value>>,
    web: PathBuf,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ScopedRequest {
    protocol: Protocol,
    plan_manifest: String,
    request: BuildRequest,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Submit {
    request: ScopedRequest,
    transaction_hash: String,
    signing_message: String,
    signature: String,
}

fn self_control_script(plan: &Plan, row: &Value) -> Option<Result<packed::Script>> {
    row["account_id"]
        .as_u64()
        .and_then(|id| u32::try_from(id).ok())
        .and_then(|id| plan.recipient_scripts.get(&id))
        .map(Script::packed)
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlProposal {
    pub protocol: Protocol,
    pub plan_manifest: String,
    pub skeleton: crate::withdraw::build::Skeleton,
    /// Ops signatures collected offline; the service has no governance keys.
    pub signatures: Vec<crate::settlement::signing::Signatures>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ControlSubmit {
    protocol: Protocol,
    plan_manifest: String,
    transaction_hash: String,
    signatures: Vec<crate::settlement::signing::Signatures>,
}
impl App {
    fn load(options: Options, protocol: Protocol) -> Result<Self> {
        ensure!(
            (1..=120).contains(&options.body_timeout_secs),
            "Body timeout must be between 1 and 120 seconds"
        );
        ensure!(
            options.confirmations > 0 && options.confirmations <= 10000,
            "Invalid confirmations"
        );
        ensure!(
            (1..=100_000).contains(&options.fee_rate),
            "Invalid fee rate"
        );
        ensure!(
            (options.public_origin.starts_with("http://")
                || options.public_origin.starts_with("https://"))
                && !options.public_origin.ends_with('/')
                && !options.public_origin.contains(['?', '#', '@']),
            "Use an exact public origin without a trailing slash"
        );
        let plan: Plan = serde_json::from_slice(&fs::read(&options.plan)?)?;
        plan.verify()?;
        ensure!(plan.protocol == protocol, "Portal plan protocol mismatch");
        let deployment: Option<withdraw::Deployment> = options
            .deployment
            .as_ref()
            .map(|p| -> Result<_> { Ok(serde_json::from_slice(&fs::read(p)?)?) })
            .transpose()?;
        if let Some(d) = &deployment {
            let profile = Profile::load(d)?;
            profile.bind(&plan)?;
        }

        for (name, digest) in &plan.input_sha256 {
            ensure!(
                [
                    "report.json",
                    "assets.json",
                    "source-allocations.json",
                    "source-checks.json",
                    "summary.json",
                    "asset-coverage.json",
                    "source-reserves.csv",
                    "claims.csv",
                    "unmapped-balances.csv",
                    "empty-successor.json",
                    "account-assets.csv"
                ]
                .contains(&name.as_str()),
                "Unexpected plan input path"
            );
            ensure!(
                hex::encode(Sha256::digest(fs::read(options.tables.join(name))?)) == *digest,
                "Plan/table hash mismatch: {name}"
            );
        }
        let generated: Option<Value> = if plan.input_sha256.contains_key("report.json") {
            Some(serde_json::from_slice(&fs::read(
                options.tables.join("report.json"),
            )?)?)
        } else {
            None
        };
        let assets: Vec<Asset> = if let Some(report) = &generated {
            crate::settlement::table::inspect(report, protocol, None, None)?;
            report["assets"]
                .as_array()
                .context("Assets")?
                .iter()
                .map(|a| {
                    Ok(Asset {
                        asset_id: u32::try_from(a["asset_id"].as_u64().context("Asset ID")?)?,
                        symbol: a["symbol"].as_str().map(String::from),
                        decimals: a["decimals"].as_u64().map(u8::try_from).transpose()?,
                        l1_decimals: if a["asset_id"] == 1 {
                            Some(8)
                        } else {
                            a["decimals"].as_u64().map(u8::try_from).transpose()?
                        },
                        type_hash: a["type_hash"].as_str().context("Asset hash")?.into(),
                    })
                })
                .collect::<Result<_>>()?
        } else if protocol == Protocol::V1 {
            serde_json::from_slice(&fs::read(options.tables.join("assets.json"))?)?
        } else {
            let rows: Vec<Value> =
                serde_json::from_slice(&fs::read(options.tables.join("asset-coverage.json"))?)?;
            rows.iter()
                .map(|a| {
                    let decimals = a["remaining_reserved_amount"]
                        .as_str()
                        .and_then(|s| s.split_once('.'))
                        .and_then(|(_, v)| u8::try_from(v.len()).ok());
                    Ok(Asset {
                        asset_id: u32::try_from(
                            a["asset_account_id"].as_u64().context("Asset ID")?,
                        )?,
                        symbol: a["symbol"].as_str().map(String::from),
                        decimals,
                        l1_decimals: decimals,
                        type_hash: a["l1_type_hash"].as_str().context("Asset type")?.into(),
                    })
                })
                .collect::<Result<_>>()?
        };
        let mut source_assets = BTreeMap::new();
        for raw in &plan.sources {
            let s: Source = crate::settlement::plan::parse(raw)?;
            source_assets.insert(source_key(hash32(&plan.deployment)?, &s), s.asset);
        }
        let mut entitlements = BTreeMap::<String, BTreeMap<u32, gw_types::U256>>::new();
        for raw in &plan.claims {
            let c: Claim = crate::settlement::plan::parse(raw)?;
            let asset = *source_assets
                .get(&c.source)
                .context("Unknown claim source")?;
            let n = entitlements
                .entry(hx(c.address))
                .or_default()
                .entry(asset)
                .or_default();
            *n = n.checked_add(c.cap).context("Entitlement overflow")?;
        }
        let mut explanations = BTreeMap::<String, Vec<Value>>::new();
        if let Some(report) = &generated {
            for row in report["accounts"].as_array().context("Accounts")? {
                let key = if let Some(script) = self_control_script(&plan, row) {
                    hx(c::control::identity(&script?))
                } else if let Some(eth) = row["owner_eth_address"].as_str() {
                    hx(eth_address(eth)?)
                } else {
                    continue;
                };
                explanations.entry(key).or_default().push(json!({"asset_id":row["asset_id"],"symbol":row["symbol"],"calculated_entitlement":row["entitlement"],"status":row["status"],"reason":row["protocols_or_reason"],"protocols":[row["protocols_or_reason"]]}));
            }
        } else if protocol == Protocol::V1 {
            let rows: Vec<Value> =
                serde_json::from_slice(&fs::read(options.tables.join("accounts.json"))?)?;
            for row in rows {
                let beneficiary = row["beneficiary"].as_str().context("Missing beneficiary")?;
                // Named pool/royalty reserves have no eligible ETH recipient. Keep them in the published tables.
                if beneficiary.starts_with("reserve:") {
                    ensure!(
                        ["RESERVED_PROTOCOL_OR_ROUNDING", "DEFER_CREATOR_ROYALTIES"]
                            .contains(&row["status"].as_str().unwrap_or_default()),
                        "Unexpected payable reserve label"
                    );
                    continue;
                }
                let key = hx(eth_address(beneficiary)?);
                explanations.entry(key).or_default().push(row);
            }
        } else {
            let data = fs::read(options.tables.join("account-assets.csv"))?;
            for row in
                csv::Reader::from_reader(data.as_slice()).deserialize::<BTreeMap<String, String>>()
            {
                let row = row?;
                let eth = row.get("owner_eth_address").context("Owner column")?;
                if eth.is_empty() {
                    continue;
                }
                explanations.entry(hx(eth_address(eth)?)).or_default().push(json!({"asset_id":row["asset_account_id"].parse::<u32>()?,"symbol":row["symbol"],"calculated_entitlement":row["known_refund_amount"],"status":row["status"],"reason":format!("{}; native holding: {} {}. V0 excludes YOK/KOI, rewards and retained protocol/control funds. Capacity-blocked rights remain unpaid.",row["interpretation"],row["native_held_amount"],row["symbol"]),"protocols":row["routes"].split('|').collect::<Vec<_>>()}));
            }
        }
        let deps: Vec<Dependency> = serde_json::from_slice(&fs::read(&options.dependencies)?)?;
        ensure!(
            !deps.is_empty() && deps.len() <= 8 && deps.iter().all(|d| d.dep_type == "code"),
            "Use 1-8 explicit code/data dependencies, not unresolved dep groups"
        );
        let rpc = Rpc::new(&options.ckb_rpc)?;
        let indexer = Rpc::new(options.indexer_rpc.as_deref().unwrap_or(&options.ckb_rpc))?;
        let web = options.web_dir.canonicalize()?;
        ensure!(web.join("index.html").is_file(), "Build the web UI first");
        Ok(Self {
            options,
            rpc,
            indexer,
            assets,
            deps,
            plan,
            deployment,
            entitlements,
            explanations,
            web,
        })
    }
    fn network(&self) -> Result<()> {
        let genesis = self.rpc.call("get_header_by_number", json!(["0x0"]))?;
        ensure!(genesis["hash"] == GENESIS, "RPC must be CKB mainnet");
        Ok(())
    }
    fn dependencies(&self) -> Result<Vec<Cell>> {
        let mut cells = Vec::new();
        let mut seen = BTreeSet::new();
        for dep in &self.deps {
            ensure!(seen.insert(dep.out_point.clone()), "Duplicate dependency");
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
            "Missing standard mainnet OmniLock"
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
    fn inventory(&self, address: &str) -> Result<Value> {
        self.network()?;
        let eth = hx(eth_address(address)?);
        let lock = owner(&eth)?;
        let tip = self.rpc.call("get_tip_header", json!([]))?;
        let tip_n = quantity(tip["number"].as_str().context("Tip unavailable")?)?;
        let indexer_tip = self.indexer.call("get_indexer_tip", json!([]))?;
        let indexer_n = quantity(
            indexer_tip["block_number"]
                .as_str()
                .context("Indexer unavailable")?,
        )?;
        ensure!(
            indexer_n <= tip_n && tip_n - indexer_n <= 20,
            "Indexer is not synchronized; try again later"
        );
        let h = self
            .rpc
            .call("get_header_by_number", json!([indexer_tip["block_number"]]))?;
        ensure!(
            h["hash"] == indexer_tip["block_hash"],
            "Indexer is on a different chain"
        );
        let search = json!({"script":lock,"script_type":"lock","script_search_mode":"exact","with_data":true});
        let mut cursor = None;
        let mut found = Vec::new();
        let mut provenance = self
            .deployment
            .as_ref()
            .map(|d| super::provenance::Provenance::new(&self.rpc, d, self.options.confirmations))
            .transpose()?;
        let mut unattributed = 0;
        let mut seen = BTreeSet::new();
        let mut ignored = 0;
        let mut pending = 0;
        let mut complete = false;
        for _ in 0..20 {
            let mut params = vec![search.clone(), json!("asc"), json!("0x64")];
            if let Some(c) = &cursor {
                params.push(json!(c));
            }
            let page = self.indexer.call("get_cells", json!(params))?;
            let cells: Vec<Cell> = serde_json::from_value(page["objects"].clone())?;
            if cells.is_empty() {
                complete = true;
                break;
            }
            for cell in cells {
                ensure!(
                    cell.output.lock == lock && seen.insert(cell.out_point.clone()),
                    "Invalid indexer response"
                );
                if !supported(&cell, &self.assets)? {
                    ignored += 1;
                    continue;
                }
                let block = quantity(&cell.block_number)?;
                if tip_n
                    .checked_sub(block)
                    .and_then(|n| n.checked_add(1))
                    .unwrap_or(0)
                    < self.options.confirmations
                {
                    pending += 1;
                    continue;
                }
                if let Some(proof) = provenance.as_mut() {
                    if proof.owns(&cell.out_point, &lock)? {
                        found.push(cell);
                    } else {
                        unattributed += 1;
                    }
                } else {
                    unattributed += 1;
                }
            }
            let next = page["last_cursor"]
                .as_str()
                .context("Missing pagination cursor")?
                .to_owned();
            ensure!(
                !next.is_empty() && cursor.as_ref() != Some(&next),
                "Indexer pagination stalled"
            );
            cursor = Some(next);
        }
        ensure!(
            complete,
            "Too many cells to enumerate safely; use operator assistance"
        );
        if let Some(proof) = &provenance {
            proof.history.recheck()?;
        }
        let more = found.len() > MAX_INPUTS;
        found.truncate(MAX_INPUTS);
        for c in &found {
            self.rpc.verify_live(c)?;
        }
        let mut available = BTreeMap::<u32, gw_types::U256>::new();
        for cell in &found {
            let n = available.entry(1).or_default();
            *n = n
                .checked_add(quantity(&cell.output.capacity)?.into())
                .context("Capacity overflow")?;
            if let Some(t) = &cell.output.type_script {
                let a = self
                    .assets
                    .iter()
                    .find(|a| t.hash().ok().as_ref() == Some(&a.type_hash))
                    .context("Unknown token")?;
                let n = available.entry(a.asset_id).or_default();
                *n = n
                    .checked_add(gw_types::U256::from_little_endian(&bytes(
                        &cell.output_data,
                    )?))
                    .context("Token overflow")?;
            }
        }
        Ok(
            json!({"protocol":self.plan.protocol,"unattributed_cells":unattributed,"address":eth,"omnilock_address":super::address(&lock)?,"snapshot_root":self.plan.snapshot_root,"plan_manifest":self.plan.manifest,"entitlements":self.entitlements.get(&eth).map(|m|m.iter().map(|(id,n)|json!({"asset_id":id,"raw":n.to_string()})).collect::<Vec<_>>()).unwrap_or_default(),"explanations":self.explanations.get(&eth).cloned().unwrap_or_default(),"available":available.iter().map(|(id,n)|json!({"asset_id":id,"raw":n.to_string()})).collect::<Vec<_>>(),"cells":found,"more_batches":more,"unsupported_cells":ignored,"confirming_cells":pending,"confirmations_required":self.options.confirmations,"observed_block":tip_n}),
        )
    }
    fn prepare(&self, scoped: &ScopedRequest) -> Result<Quote> {
        ensure!(
            scoped.protocol == self.plan.protocol && scoped.plan_manifest == self.plan.manifest,
            "Selected protocol or plan changed; reload and review again"
        );
        let r = &scoped.request;
        let d = self
            .deployment
            .as_ref()
            .context("Distribution release not configured; lookup only")?;
        let mut provenance =
            super::provenance::Provenance::new(&self.rpc, d, self.options.confirmations)?;
        let owner = owner(&r.address)?;
        ensure!(
            !r.inputs.is_empty() && r.inputs.len() <= MAX_INPUTS,
            "Invalid input count"
        );
        self.network()?;
        let deps = self.dependencies()?;
        let mut history = History::new(&self.rpc, self.options.confirmations)?;
        let mut inputs = Vec::new();
        for p in &r.inputs {
            let c = chain::live(&self.rpc, p)?;
            let committed = history.cell(p)?;
            ensure!(
                chain::same_cell(&c, &committed),
                "Input differs from confirmed chain history"
            );
            ensure!(
                provenance.owns(p, &owner)?,
                "Input is not an unmixed refund cell from the selected protocol release"
            );
            inputs.push(c);
        }
        let q = construct(r, inputs, &self.assets, &self.deps, self.options.fee_rate)?;
        withdraw::verify_script_dependencies(&q.input_cells, &q.transaction.outputs, &deps)?;
        history.recheck()?;
        provenance.history.recheck()?;
        Ok(q)
    }
    fn submit(&self, s: Submit) -> Result<Value> {
        ensure!(
            self.options.enable_broadcast,
            "Transfers are not enabled by the operator yet"
        );
        let quote = self.prepare(&s.request)?;
        ensure!(
            quote.transaction_hash == s.transaction_hash
                && quote.signing_message == s.signing_message,
            "Transaction changed; review and sign a new quote"
        );
        let tx = assemble(&quote, &s.signature)?;
        let cycle = self.rpc.call("estimate_cycles", json!([tx]))?;
        let cycles = quantity(
            cycle["cycles"]
                .as_str()
                .context("Node cannot validate all scripts")?,
        )?;
        ensure!(cycles <= MAX_CYCLES, "Transaction exceeds the cycle budget");
        self.dependencies()?;
        for c in &quote.input_cells {
            self.rpc.verify_live(c)?;
        }
        let result = broadcast(&self.rpc.endpoint, &tx)?;
        ensure!(result.as_str()==Some(quote.transaction_hash.as_str()),"Unexpected transaction hash from node; check the reviewed transaction hash before retrying");
        Ok(json!({"transaction_hash":quote.transaction_hash,"status":"submitted","cycles":cycles}))
    }
    fn controls(&self) -> Result<Value> {
        let accounts=self.plan.recipient_scripts.iter().map(|(id,s)|{
            let script=s.packed()?;let a=script.args().raw_data();let key=hx(c::control::identity(&script));
            Ok(json!({"account_id":id,"threshold":a[36],"owners":a[38..].chunks_exact(20).map(hx).collect::<Vec<_>>(),"identity":key,"descriptor":s,"entitlements":self.entitlements.get(&key).map(|v|v.iter().map(|(id,n)|json!({"asset_id":id,"raw":n.to_string()})).collect::<Vec<_>>()).unwrap_or_default(),"explanations":self.explanations.get(&key)}))
        }).collect::<Result<Vec<_>>>()?;
        Ok(
            json!({"protocol":self.plan.protocol,"plan_manifest":self.plan.manifest,"accounts":accounts,"proposal_service_enabled":self.options.control_proposals.is_some()}),
        )
    }
    fn load_control(
        &self,
        id: &str,
    ) -> Result<(ControlProposal, crate::settlement::signing::SigningRequest)> {
        let id = hx(hash32(id)?);
        let root = self
            .options
            .control_proposals
            .as_ref()
            .context("Control proposals not configured")?;
        let file = root
            .join(self.plan.protocol.name())
            .join(format!("{id}.json"));
        ensure!(
            fs::metadata(&file)?.len() <= 8 * 1024 * 1024,
            "Oversized proposal"
        );
        let p: ControlProposal = serde_json::from_slice(&fs::read(file)?)?;
        ensure!(
            p.protocol == self.plan.protocol
                && p.plan_manifest == self.plan.manifest
                && p.skeleton.transaction.hash()? == id,
            "Control proposal scope mismatch"
        );
        let d = self.deployment.as_ref().context("Deployment absent")?;
        let profile = Profile::load(d)?;
        profile.bind(&self.plan)?;
        let action = crate::settlement::replay::action_for(&p.skeleton.transaction, &profile)?;
        ensure!(
            action.op == c::settlement::Operation::Pay && action.epoch == 0,
            "Control proposal must pay the published epoch"
        );
        let mut recipients = BTreeSet::new();
        for raw in &action.records {
            let pay: Payment = decode(raw).map_err(|e| anyhow::anyhow!("{e:?}"))?;
            ensure!(
                self.plan.claims.contains(&hx(encode(&pay.claim))),
                "Proposal claim outside published plan"
            );
            ensure!(
                self.plan
                    .recipient_scripts
                    .contains_key(&pay.claim.recipient),
                "Web proposal includes a non-control recipient"
            );
            recipients.insert(pay.claim.recipient);
        }
        ensure!(
            recipients.len() == 1,
            "Web proposal must contain exactly one control recipient"
        );
        let req = crate::settlement::signing::requests(d, &p.skeleton)?;
        crate::settlement::signing::verify_ops_signatures(&req, &p.signatures)?;
        for cell in p
            .skeleton
            .input_cells
            .iter()
            .chain(&p.skeleton.dependency_cells)
        {
            self.rpc.verify_live(cell)?;
        }
        Ok((p, req))
    }
    fn control_proposal(&self, id: &str) -> Result<Value> {
        let (p, req) = self.load_control(id)?;
        let profile = Profile::load(self.deployment.as_ref().context("Deployment")?)?;
        let action = crate::settlement::replay::action_for(&p.skeleton.transaction, &profile)?;
        let accounts = action
            .native
            .recipients
            .iter()
            .filter(|r| c::control::is_descriptor(&r.script))
            .map(|r| r.id)
            .collect::<Vec<_>>();
        Ok(
            json!({"protocol":self.plan.protocol,"plan_manifest":self.plan.manifest,"control_accounts":accounts,"proposal":p,"signing_request":req}),
        )
    }
    fn control_submit(&self, s: ControlSubmit) -> Result<Value> {
        ensure!(
            self.options.enable_broadcast
                && s.protocol == self.plan.protocol
                && s.plan_manifest == self.plan.manifest,
            "Control submission disabled or wrong scope"
        );
        let (mut p, _) = self.load_control(&s.transaction_hash)?;
        p.signatures.extend(s.signatures);
        let tx = crate::settlement::signing::assemble(
            self.deployment.as_ref().context("Deployment")?,
            &p.skeleton,
            &p.signatures,
        )?;
        let result = self.rpc.call("estimate_cycles", json!([tx]))?;
        let cycles = quantity(result["cycles"].as_str().context("Cycles")?)?;
        ensure!(
            cycles <= MAX_CYCLES,
            "Control transaction exceeds cycle budget"
        );
        self.load_control(&s.transaction_hash)?;
        let result = broadcast(&self.rpc.endpoint, &tx)?;
        ensure!(
            result.as_str() == Some(s.transaction_hash.as_str()),
            "Unexpected broadcast result; reconcile original transaction hash"
        );
        Ok(json!({"transaction_hash":s.transaction_hash,"status":"submitted","cycles":cycles}))
    }
    fn route(&self, request: &mut Request) -> Result<(Vec<u8>, &'static str)> {
        let url = request.url();
        let prefix = format!("/api/{}", self.plan.protocol.name());
        let path = if let Some(rest) = url.strip_prefix(&prefix) {
            format!("/api{rest}")
        } else {
            url.to_owned()
        };
        let expected_host = self
            .options
            .public_origin
            .split_once("://")
            .context("Invalid origin")?
            .1;
        let host = request
            .headers()
            .iter()
            .find(|h| h.field.equiv("Host"))
            .map(|h| h.value.as_str());
        ensure!(host == Some(expected_host), "Invalid request host");
        if *request.method() == Method::Post {
            let origin = request
                .headers()
                .iter()
                .find(|h| h.field.equiv("Origin"))
                .map(|h| h.value.as_str());
            ensure!(
                origin == Some(self.options.public_origin.as_str()),
                "Cross-origin requests are not accepted"
            );
            let content = request
                .headers()
                .iter()
                .find(|h| h.field.equiv("Content-Type"))
                .map(|h| h.value.as_str());
            ensure!(
                content.is_some_and(|s| s.starts_with("application/json")),
                "Use application/json"
            );
        }
        let value = match (request.method(), path.as_str()) {
            (&Method::Get, "/api/config") => {
                json!({"network":"CKB mainnet","scope":format!("Godwoken {}",self.plan.protocol.name()),"protocol":self.plan.protocol,"assets":self.assets,"dependencies":self.deps,"broadcast_enabled":self.options.enable_broadcast && self.deployment.is_some(),"fee_rate":self.options.fee_rate,"plan_manifest":self.plan.manifest,"snapshot_root":self.plan.snapshot_root})
            }
            (&Method::Get, "/api/controls") => self.controls()?,
            (&Method::Get, p) if p.starts_with("/api/control-proposal/") => {
                self.control_proposal(&p[22..])?
            }
            (&Method::Post, "/api/control-submit") => {
                self.control_submit(read_json(request, self.options.body_timeout_secs)?)?
            }
            (&Method::Get, p) if p.starts_with("/api/account/") => self.inventory(&p[13..])?,
            (&Method::Get, p) if p.starts_with("/api/transaction/") => {
                let h = hx(hash32(&p[17..])?);
                let result = self.rpc.call("get_transaction", json!([h]))?;
                json!({"transaction_hash":h,"status":result["tx_status"]["status"],"block_hash":result["tx_status"]["block_hash"]})
            }
            (&Method::Post, "/api/prepare") => {
                let mut q = serde_json::to_value(self.prepare(&read_json::<ScopedRequest>(
                    request,
                    self.options.body_timeout_secs,
                )?)?)?;
                q["protocol"] = json!(self.plan.protocol);
                q["plan_manifest"] = json!(self.plan.manifest);
                q
            }
            (&Method::Post, "/api/submit") => {
                self.submit(read_json(request, self.options.body_timeout_secs)?)?
            }
            (&Method::Get, p) if !p.starts_with("/api/") => {
                let file = if p == "/" {
                    self.web.join("index.html")
                } else {
                    self.web.join(p.trim_start_matches('/'))
                };
                let file = file.canonicalize().context("Page not found")?;
                ensure!(
                    file.starts_with(&self.web) && file.is_file(),
                    "Page not found"
                );
                let mime = match file.extension().and_then(|x| x.to_str()) {
                    Some("html") => "text/html; charset=utf-8",
                    Some("js") => "text/javascript; charset=utf-8",
                    Some("css") => "text/css; charset=utf-8",
                    Some("svg") => "image/svg+xml",
                    Some("json") => "application/json",
                    _ => "application/octet-stream",
                };
                return Ok((fs::read(file)?, mime));
            }
            _ => anyhow::bail!("Unknown route"),
        };
        Ok((serde_json::to_vec(&value)?, "application/json"))
    }
}
#[derive(Debug)]
struct BodyReadTimeout;
impl std::fmt::Display for BodyReadTimeout {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Request body read timed out")
    }
}
impl std::error::Error for BodyReadTimeout {}

fn read_json<T: serde::de::DeserializeOwned>(r: &mut Request, timeout_secs: u64) -> Result<T> {
    let length = r
        .body_length()
        .context("Request body requires Content-Length")?;
    ensure!(length <= 65536, "Request body limit is 64 KiB");
    let deadline = Instant::now() + Duration::from_secs(timeout_secs);
    let mut body = Vec::with_capacity(length);
    let mut chunk = [0u8; 8192];
    while body.len() < length {
        if Instant::now() >= deadline {
            return Err(BodyReadTimeout.into());
        }
        let remaining = (length - body.len()).min(chunk.len());
        match r.as_reader().read(&mut chunk[..remaining]) {
            Ok(0) => anyhow::bail!("Request body ended before Content-Length"),
            Ok(n) => {
                if Instant::now() >= deadline {
                    return Err(BodyReadTimeout.into());
                }
                body.extend_from_slice(&chunk[..n]);
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
                ) =>
            {
                return Err(BodyReadTimeout.into());
            }
            Err(e) => return Err(e.into()),
        }
    }
    Ok(serde_json::from_slice(&body)?)
}
/// Deliberately separate from `network::Rpc` so audit/replay commands retain their read-only guarantee.
fn broadcast(endpoint: &str, tx: &Transaction) -> Result<Value> {
    let mut child = Command::new("curl")
        .args([
            "-q",
            "--silent",
            "--show-error",
            "--fail-with-body",
            "--max-time",
            "30",
            endpoint,
            "-H",
            "Content-Type: application/json",
            "--data-binary",
            "@-",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    child.stdin.take().context("RPC stdin unavailable")?.write_all(&serde_json::to_vec(&json!({"jsonrpc":"2.0","id":1,"method":"send_transaction","params":[tx,"passthrough"]}))?)?;
    let out = child.wait_with_output()?;
    ensure!(
        out.status.success(),
        "Broadcast response unavailable; check the reviewed transaction hash before retrying"
    );
    let v: Value = serde_json::from_slice(&out.stdout)?;
    ensure!(
        v["id"] == 1 && v.get("error").is_none(),
        "Node rejected the claim transaction"
    );
    Ok(v["result"].clone())
}
fn http_server(listen: &str, body_timeout_secs: u64) -> Result<Server> {
    // Apply this to accepted sockets, not the listener: a listener read timeout
    // would also time out accept() and stop tiny_http's accept loop.
    let idle_timeout = Duration::from_secs(body_timeout_secs.min(3));
    Server::http_with_timeout(listen, idle_timeout).map_err(|e| anyhow::anyhow!("HTTP server: {e}"))
}

pub fn run(options: Options) -> Result<()> {
    let mut versions = BTreeMap::new();
    if let (Some(plan), Some(tables)) = (&options.v0_plan, &options.v0_tables) {
        let mut v0 = options.clone();
        v0.plan = plan.clone();
        v0.tables = tables.clone();
        v0.deployment = options.v0_deployment.clone();
        versions.insert("v0", App::load(v0, Protocol::V0)?);
    }
    let app = Arc::new(App::load(options, Protocol::V1)?);
    let versions = Arc::new(versions);
    let server = Arc::new(http_server(
        &app.options.listen,
        app.options.body_timeout_secs,
    )?);
    eprintln!(
        "Godwoken selected-protocol withdraw portal: {} (broadcast enabled: {})",
        app.options.public_origin, app.options.enable_broadcast
    );
    std::thread::scope(|scope| {
        for _ in 0..4 {
            let app = Arc::clone(&app);
            let server = Arc::clone(&server);
            let versions = Arc::clone(&versions);
            scope.spawn(move|| {
            for mut request in server.incoming_requests() {
                let result=if request.url().starts_with("/api/v0/") {
                    match versions.get("v0") {Some(v)=>v.route(&mut request),None=>Err(anyhow::anyhow!("V0 refund release is not configured"))}
                } else if request.url().starts_with("/api/") && !request.url().starts_with("/api/v1/") {Err(anyhow::anyhow!("Select an explicit /api/v1/ or /api/v0/ endpoint"))}
                else {app.route(&mut request)};
                let (body,mime,status)=match result {Ok((b,m))=>(b,m,200),Err(e)=>{
                    let status=if e.downcast_ref::<BodyReadTimeout>().is_some(){408}else{400};
                    (serde_json::to_vec(&json!({"error":e.to_string()})).unwrap(),"application/json",status)
                }};
                let mut response=Response::from_data(body).with_status_code(StatusCode(status));
                if status==408 {response.add_header(Header::from_bytes("Connection","close").unwrap());}
                for (name,value) in [("Content-Type",mime),("Cache-Control","no-store"),("X-Content-Type-Options","nosniff"),("Referrer-Policy","no-referrer"),("Content-Security-Policy","default-src 'self'; script-src 'self'; style-src 'self'; connect-src 'self'; img-src 'self' data:; frame-ancestors 'none'; base-uri 'none'; form-action 'none'")] {response.add_header(Header::from_bytes(name,value).unwrap());}
                let _=request.respond(response);
            }
        });
        }
    });
    Ok(())
}

#[cfg(all(test, unix))]
mod timeout_tests {
    use super::*;
    use std::{
        io::{BufRead, BufReader},
        net::TcpStream,
        sync::atomic::{AtomicBool, Ordering},
    };

    fn status(stream: TcpStream) -> u16 {
        let mut line = String::new();
        BufReader::new(stream).read_line(&mut line).unwrap();
        line.split_whitespace().nth(1).unwrap().parse().unwrap()
    }

    #[test]
    fn incomplete_bodies_expire_and_workers_recover() {
        let server = Arc::new(http_server("127.0.0.1:0", 1).unwrap());
        let address = server.server_addr().to_ip().unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let workers: Vec<_> = (0..4)
            .map(|_| {
                let server = Arc::clone(&server);
                let stop = Arc::clone(&stop);
                std::thread::spawn(move || {
                    while !stop.load(Ordering::Relaxed) {
                        let Ok(Some(mut request)) = server.recv_timeout(Duration::from_millis(100))
                        else {
                            continue;
                        };
                        let status = if *request.method() == Method::Post {
                            match read_json::<Value>(&mut request, 1) {
                                Ok(_) => 200,
                                Err(e) if e.downcast_ref::<BodyReadTimeout>().is_some() => 408,
                                Err(_) => 400,
                            }
                        } else {
                            200
                        };
                        let response =
                            Response::from_string("ok").with_status_code(StatusCode(status));
                        let _ = request.respond(response);
                    }
                })
            })
            .collect();

        let mut blocked = Vec::new();
        for _ in 0..4 {
            let mut stream = TcpStream::connect(address).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            stream.write_all(b"POST /api/test HTTP/1.1\r\nHost: localhost\r\nContent-Length: 2048\r\nConnection: close\r\n\r\n{").unwrap();
            blocked.push(stream);
        }
        for stream in blocked {
            assert_eq!(status(stream), 408);
        }

        let mut get = TcpStream::connect(address).unwrap();
        get.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        get.write_all(b"GET /api/test HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
            .unwrap();
        assert_eq!(status(get), 200);

        let mut post = TcpStream::connect(address).unwrap();
        post.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        post.write_all(b"POST /api/test HTTP/1.1\r\nHost: localhost\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}").unwrap();
        assert_eq!(status(post), 200);
        stop.store(true, Ordering::Relaxed);
        for worker in workers {
            worker.join().unwrap();
        }
    }
}
