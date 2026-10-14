use crate::model::*;
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Write,
    process::{Command, Stdio},
};

#[derive(Clone)]
pub struct Rpc {
    pub endpoint: String,
}
impl Rpc {
    pub fn new(endpoint: &str) -> Result<Self> {
        ensure!(
            endpoint.starts_with("https://") || endpoint.starts_with("http://"),
            "RPC URL must be HTTP(S)"
        );
        Ok(Self {
            endpoint: endpoint.into(),
        })
    }
    pub fn call(&self, method: &str, params: Value) -> Result<Value> {
        ensure!(
            [
                "get_cells",
                "get_live_cell",
                "get_indexer_tip",
                "get_header_by_number",
                "get_header",
                "get_tip_header",
                "get_transaction",
                "estimate_cycles"
            ]
            .contains(&method),
            "RPC method is outside the read-only allowlist"
        );
        let mut child = Command::new("curl")
            .args([
                "-q",
                "--silent",
                "--show-error",
                "--fail-with-body",
                "--max-time",
                "30",
                &self.endpoint,
                "-H",
                "Content-Type: application/json",
                "--data-binary",
                "@-",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("Start curl (read-only JSON RPC)")?;
        child
            .stdin
            .take()
            .context("RPC stdin unavailable")?
            .write_all(&serde_json::to_vec(
                &json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}),
            )?)?;
        let out = child.wait_with_output()?;
        ensure!(
            out.status.success(),
            "{method}: transport error: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let response: Value =
            serde_json::from_slice(&out.stdout).context("Non-JSON RPC response")?;
        ensure!(
            response.get("id") == Some(&json!(1))
                && response.get("error").is_none()
                && response.get("result").is_some(),
            "{method}: RPC error {response}"
        );
        Ok(response["result"].clone())
    }
    pub fn cells(&self, script: &Script, kind: &str, mode: &str) -> Result<Vec<Cell>> {
        let search =
            json!({"script":script,"script_type":kind,"script_search_mode":mode,"with_data":true});
        let mut all = Vec::new();
        let mut seen = BTreeSet::new();
        let mut cursor = None;
        for _ in 0..10000 {
            let mut params = vec![search.clone(), json!("asc"), json!("0x64")];
            if let Some(c) = &cursor {
                params.push(json!(c));
            }
            let page = self.call("get_cells", json!(params))?;
            let cells: Vec<Cell> = serde_json::from_value(
                page.get("objects")
                    .context("Missing indexer objects")?
                    .clone(),
            )?;
            if cells.is_empty() {
                return Ok(all);
            }
            for cell in cells {
                let actual = if kind == "lock" {
                    &cell.output.lock
                } else {
                    cell.output
                        .type_script
                        .as_ref()
                        .context("Missing type Script")?
                };
                ensure!(
                    actual.code_hash == script.code_hash && actual.hash_type == script.hash_type,
                    "Wrong Script returned by indexer"
                );
                ensure!(
                    if mode == "exact" {
                        actual.args == script.args
                    } else {
                        bytes(&actual.args)?.starts_with(&bytes(&script.args)?)
                    },
                    "Wrong indexer args"
                );
                ensure!(
                    seen.insert(cell.out_point.clone()),
                    "Indexer repeated an outpoint"
                );
                all.push(cell);
            }
            let next = page["last_cursor"]
                .as_str()
                .context("Missing indexer cursor")?
                .to_owned();
            ensure!(
                !next.is_empty() && cursor.as_ref() != Some(&next),
                "Indexer cursor did not advance"
            );
            cursor = Some(next);
        }
        anyhow::bail!("Indexer page limit reached; refusing truncated inventory")
    }
    pub fn verify_live(&self, c: &Cell) -> Result<()> {
        let current = self.call("get_live_cell", json!([c.out_point, true]))?;
        ensure!(
            current["status"] == "live"
                && current["cell"]["data"]["content"] == c.output_data
                && current["cell"]["output"] == serde_json::to_value(&c.output)?,
            "Cell is spent or content differs from indexer"
        );
        Ok(())
    }
}
fn scan(idx: &Rpc, cfg: &Config) -> Result<BTreeMap<String, Vec<Cell>>> {
    std::thread::scope(|scope| {
        let jobs: Vec<_> = FAMILIES
            .iter()
            .map(|family| {
                scope.spawn(move || {
                    let script = Script {
                        code_hash: cfg.hashes[*family].clone(),
                        hash_type: "type".into(),
                        args: ROLLUP_HASH.into(),
                    };
                    Ok((family.to_string(), idx.cells(&script, "lock", "prefix")?))
                })
            })
            .collect();
        jobs.into_iter()
            .map(|j| {
                j.join()
                    .map_err(|_| anyhow::anyhow!("Indexer worker panicked"))?
            })
            .collect()
    })
}
pub fn config_outpoint(s: Option<&str>) -> Result<OutPoint> {
    let default = "0xd891e0e8c4864c730cc4b96b450ed1bcaf42e74c9eb1a38fb193caaf490f9a3d:0x0";
    let (tx, index) = s
        .unwrap_or(default)
        .rsplit_once(':')
        .context("Expected tx_hash:0xindex")?;
    hash32(tx)?;
    ensure!(
        quantity(index)? <= u32::MAX as u64,
        "Invalid config output index"
    );
    Ok(OutPoint {
        tx_hash: tx.to_owned(),
        index: format!("0x{:x}", quantity(index)?),
    })
}
pub fn capture(ckb_url: &str, indexer_url: &str, outpoint: OutPoint) -> Result<Bundle> {
    let rpc = Rpc::new(ckb_url)?;
    let idx = Rpc::new(indexer_url)?;
    let start = now();
    let before = idx.call("get_indexer_tip", json!([]))?;
    ensure!(
        before["block_hash"].is_string() && before["block_number"].is_string(),
        "Indexer has no tip"
    );
    let header = rpc.call("get_header_by_number", json!([before["block_number"]]))?;
    ensure!(
        header["hash"] == before["block_hash"],
        "CKB/indexer canonical chain mismatch"
    );
    let rollups = idx.cells(&rollup_script(), "type", "exact")?;
    ensure!(rollups.len() == 1, "Expected exactly one live rollup");
    let rollup = &rollups[0];
    let state = GlobalState::parse(&bytes(&rollup.output_data)?)?;
    let config = rpc.call("get_live_cell", json!([outpoint, true]))?;
    ensure!(
        config["status"] == "live",
        "Config dependency is not live; provide --config-outpoint"
    );
    let config_data = config["cell"]["data"]["content"]
        .as_str()
        .context("Missing config data")?
        .to_owned();
    ensure!(
        hx(hash(&bytes(&config_data)?)) == state.config_hash,
        "Config changed; provide correct --config-outpoint"
    );
    let cfg = Config::parse(&bytes(&config_data)?)?;
    let cells = scan(&idx, &cfg)?;
    let again = scan(&idx, &cfg)?;
    ensure!(cells == again, "L1 inventory changed between scans; rerun");
    ensure!(
        rollups == idx.cells(&rollup_script(), "type", "exact")?,
        "Rollup changed during capture"
    );
    rpc.verify_live(rollup)?;
    let all: Vec<_> = cells.values().flatten().collect();
    for group in all.chunks(4) {
        std::thread::scope(|scope| -> Result<()> {
            let jobs: Vec<_> = group
                .iter()
                .map(|c| scope.spawn(|| rpc.verify_live(c)))
                .collect();
            for j in jobs {
                j.join()
                    .map_err(|_| anyhow::anyhow!("Liveness worker panicked"))??;
            }
            Ok(())
        })?;
    }
    let after = idx.call("get_indexer_tip", json!([]))?;
    let end_header = rpc.call("get_header_by_number", json!([after["block_number"]]))?;
    let old_header = rpc.call("get_header_by_number", json!([before["block_number"]]))?;
    ensure!(
        end_header["hash"] == after["block_hash"] && old_header["hash"] == before["block_hash"],
        "Canonicality changed during capture"
    );
    ensure!(
        quantity(after["block_number"].as_str().context("Invalid end tip")?)?
            >= quantity(
                before["block_number"]
                    .as_str()
                    .context("Invalid start tip")?
            )?,
        "Indexer moved backwards"
    );
    ensure!(
        rollups == idx.cells(&rollup_script(), "type", "exact")?,
        "Rollup changed during liveness checks"
    );
    let bundle = Bundle {
        schema_version: 1,
        network: "mainnet_v1".into(),
        observed_start: start,
        observed_end: now(),
        rollup_cell: rollup.clone(),
        config_data,
        live_cells_checked: all.len(),
        cells,
        consistency: "double_scan_live_cells_and_stable_rollup_single_provider".into(),
        ckb_rpc: Some(ckb_url.into()),
        indexer_rpc: Some(indexer_url.into()),
        indexer_before: before,
        indexer_after: after,
    };
    bundle.validate()?;
    Ok(bundle)
}
