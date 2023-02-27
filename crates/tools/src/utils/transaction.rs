//! Transaction related utils
//! NOTICE: Some functions should be moved to a more proper module than this.

use std::{
    env,
    ffi::OsStr,
    fs,
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

use anyhow::{anyhow, bail, Result};
use ckb_fixed_hash::{h256, H256};
use gw_config::Config;
use gw_jsonrpc_types::godwoken::TxReceipt;
use gw_rpc_client::ckb_client::CkbClient;

use crate::{godwoken_rpc::GodwokenRpcClient, utils::sdk::NetworkType};

// "TYPE_ID" in hex
pub const TYPE_ID_CODE_HASH: H256 = h256!("0x545950455f4944");

pub fn run_in_dir<I, S>(bin: &str, args: I, target_dir: &str) -> Result<()>
where
    I: IntoIterator<Item = S> + std::fmt::Debug,
    S: AsRef<OsStr>,
{
    let working_dir = env::current_dir().expect("get working dir");
    env::set_current_dir(&target_dir).expect("set target dir");
    let result = run(bin, args);
    env::set_current_dir(&working_dir).expect("set working dir");
    result
}

pub fn run<I, S>(bin: &str, args: I) -> Result<()>
where
    I: IntoIterator<Item = S> + std::fmt::Debug,
    S: AsRef<OsStr>,
{
    log::debug!("[Execute]: {} {:?}", bin, args);
    let status = Command::new(bin)
        .env("RUST_BACKTRACE", "full")
        .args(args)
        .status()
        .expect("run command");
    if !status.success() {
        Err(anyhow::anyhow!(
            "Exited with status code: {:?}",
            status.code()
        ))
    } else {
        Ok(())
    }
}

pub fn run_cmd<I, S>(args: I) -> Result<String>
where
    I: IntoIterator<Item = S> + std::fmt::Debug,
    S: AsRef<OsStr>,
{
    let bin = "ckb-cli";
    log::debug!("[Execute]: {} {:?}", bin, args);
    let init_output = Command::new(bin)
        .env("RUST_BACKTRACE", "full")
        .env("RUST_LOG", "warn")
        .args(args)
        .stderr(Stdio::inherit())
        .output()
        .expect("Run command failed");

    if !init_output.status.success() {
        bail!("command exited with status {}", init_output.status)
    } else {
        let stdout = String::from_utf8_lossy(init_output.stdout.as_slice()).to_string();
        log::debug!("stdout: {}", stdout);
        Ok(stdout)
    }
}

pub async fn get_network_type(rpc_client: &CkbClient) -> Result<NetworkType> {
    let chain_info = rpc_client.get_blockchain_info().await?;
    NetworkType::from_raw_str(chain_info.chain.as_str())
        .ok_or_else(|| anyhow!("Unexpected network type: {}", chain_info.chain))
}

pub fn run_in_output_mode<I, S>(bin: &str, args: I) -> Result<(String, String)>
where
    I: IntoIterator<Item = S> + std::fmt::Debug,
    S: AsRef<OsStr>,
{
    log::debug!("[Execute]: {} {:?}", bin, args);
    let init_output = Command::new(bin)
        .env("RUST_BACKTRACE", "full")
        .args(args)
        .output()
        .expect("Run command failed");

    if !init_output.status.success() {
        Err(anyhow!(
            "{}",
            String::from_utf8_lossy(init_output.stderr.as_slice())
        ))
    } else {
        let stdout = String::from_utf8_lossy(init_output.stdout.as_slice()).to_string();
        let stderr = String::from_utf8_lossy(init_output.stderr.as_slice()).to_string();
        log::debug!("stdout: {}", stdout);
        log::debug!("stderr: {}", stderr);
        Ok((stdout, stderr))
    }
}

// Read config.toml
pub fn read_config<P: AsRef<Path>>(path: P) -> Result<Config> {
    let content = fs::read(&path)?;
    let config = toml_edit::de::from_slice(&content)?;
    Ok(config)
}

pub async fn wait_for_l2_tx(
    godwoken_rpc_client: &mut GodwokenRpcClient,
    tx_hash: &H256,
    timeout_secs: u64,
    quiet: bool,
) -> Result<Option<TxReceipt>> {
    let retry_timeout = Duration::from_secs(timeout_secs);
    let start_time = Instant::now();
    while start_time.elapsed() < retry_timeout {
        std::thread::sleep(Duration::from_secs(2));

        let receipt = godwoken_rpc_client.get_transaction_receipt(tx_hash).await?;

        match receipt {
            Some(_) => {
                if !quiet {
                    log::info!("tx committed");
                }
                return Ok(receipt);
            }
            None => {
                if !quiet {
                    log::info!("waiting for {} secs.", start_time.elapsed().as_secs());
                }
            }
        }
    }
    Err(anyhow!("Timeout: {:?}", retry_timeout))
}
