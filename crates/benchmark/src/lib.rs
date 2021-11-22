pub(crate) mod batch;
pub mod config;
pub(crate) mod plan;
pub(crate) mod polyman;
pub mod stats;
pub mod tx;

use std::fs;
use std::{cmp, env::current_dir, path::Path, str::FromStr, time::Duration};

use anyhow::anyhow;
use anyhow::Result;
use ckb_fixed_hash::H256;
use tokio::{sync::mpsc, time};

use crate::{plan::GodwokenConfig, stats::StatsHandler};

const GENERATE_CONFIG_FILE_PATH: &str = "./gw_benchmark_config.toml";
pub fn generate_config_file(path: Option<&str>) -> Result<()> {
    let config = config::Config::default();
    let path = path.unwrap_or(GENERATE_CONFIG_FILE_PATH);
    let path = current_dir()?.join(path);
    let content = toml::to_string(&config)?;
    log::debug!("content: {}", &content);
    let _ = fs::write(&path, content)?;
    log::info!("Generate benchmark config file in: {:?}", &path);
    Ok(())
}

fn read_config(path: impl AsRef<Path>) -> Result<config::Config> {
    let content = fs::read_to_string(path)?;
    let config = toml::from_str(&content)?;
    Ok(config)
}

pub async fn run(path: Option<&str>) -> Result<()> {
    let path = path.unwrap_or(GENERATE_CONFIG_FILE_PATH);
    let config = read_config(path)?;
    let pks: Vec<H256> = std::fs::read_to_string(config.account_path)?
        .split('\n')
        .map(|line| {
            H256::from_str(line.trim().trim_start_matches("0x"))
                .map_err(|err| anyhow!("parse private key with error: {:?}", err))
        })
        .collect::<Result<Vec<H256>>>()?;
    log::info!("Read private keys: {}", pks.len());
    let gw_rpc_url = reqwest::Url::parse(&config.gw_rpc_url)?;
    let polyman_url = reqwest::Url::parse(&config.polyman_url)?;
    let scripts_deployment_content = std::fs::read_to_string(&config.scripts_deploy_path)?;
    let scripts_deployment = serde_json::from_str(&scripts_deployment_content)?;
    let rollup_type_hash = H256::from_str(&config.rollup_type_hash)?;

    let stats_handler = StatsHandler::new();

    let transfer_handler = crate::tx::TxHandler::new(
        config.timeout,
        gw_rpc_url.clone(),
        polyman_url,
        rollup_type_hash.clone(),
        stats_handler.clone(),
    )
    .await?;

    let stats_fut = async move {
        let mut interval = time::interval(Duration::from_secs(10));
        loop {
            interval.tick().await;
            if let Ok(stats) = stats_handler.get_stats().await {
                log::info!("stats: {:#?}", stats);
            }
        }
    };

    let req_batch_cnt = config.batch as usize;
    let buffer = cmp::max(pks.len() / req_batch_cnt, 200);
    log::info!("batch channel buffer: {}", buffer);
    let (batch_res_sender, batch_res_receiver) = mpsc::channel(buffer);
    let batch_handler = batch::BatchHandler::new(transfer_handler, batch_res_sender);
    let gw_config = GodwokenConfig {
        scripts_deployment,
        url: gw_rpc_url,
        rollup_type_hash,
    };
    let mut plan = plan::Plan::new(
        config.interval,
        pks,
        gw_config,
        req_batch_cnt,
        batch_handler,
        batch_res_receiver,
    )
    .await;
    tokio::spawn(stats_fut);
    plan.run().await;
    Ok(())
}
