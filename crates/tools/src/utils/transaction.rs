//! Transaction related utils
//! NOTICE: Some functions should be moved to a more proper module than this.

use std::{
    env,
    ffi::OsStr,
    fs,
    path::Path,
    process::Command,
    time::{Duration, Instant},
};

use anyhow::{anyhow, Result};
use ckb_fixed_hash::H256;
use gw_config::Config;
use gw_jsonrpc_types::godwoken::TxReceipt;

use crate::godwoken_rpc::GodwokenRpcClient;

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
    let content = fs::read_to_string(&path)?;
    let config = toml::from_str(&content)?;
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
