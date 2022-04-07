//! Transaction related utils
//! NOTICE: Some functions should be moved to a more proper module than this.

use anyhow::anyhow;
use anyhow::Result;
use ckb_fixed_hash::{h256, H256};
use ckb_jsonrpc_types::Status;
use ckb_sdk::rpc::TransactionView;
use ckb_sdk::HttpRpcClient;
use ckb_sdk::NetworkType;
use gw_config::Config;
use gw_jsonrpc_types::godwoken::TxReceipt;
use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};
use std::{env, ffi::OsStr, process::Command};

use crate::godwoken_rpc::GodwokenRpcClient;

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
    let status = Command::new(bin.to_owned())
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
    let init_output = Command::new(bin.to_owned())
        .env("RUST_BACKTRACE", "full")
        .args(args)
        .output()
        .expect("Run command failed");

    if !init_output.status.success() {
        Err(anyhow!(String::from_utf8_lossy(
            init_output.stderr.as_slice()
        )
        .to_string()))
    } else {
        let stdout = String::from_utf8_lossy(init_output.stdout.as_slice()).to_string();
        log::debug!("stdout: {}", stdout);
        Ok(stdout)
    }
}

pub fn wait_for_tx(
    rpc_client: &mut HttpRpcClient,
    tx_hash: &H256,
    timeout_secs: u64,
) -> Result<TransactionView> {
    log::info!("waiting tx {}", hex::encode(&tx_hash));
    let retry_timeout = Duration::from_secs(timeout_secs);
    let start_time = Instant::now();
    while start_time.elapsed() < retry_timeout {
        std::thread::sleep(Duration::from_secs(5));
        match rpc_client.get_transaction(tx_hash.clone()) {
            Ok(Some(tx_with_status)) if tx_with_status.tx_status.status == Status::Pending => {
                log::info!("tx pending");
            }
            Ok(Some(tx_with_status)) if tx_with_status.tx_status.status == Status::Proposed => {
                log::info!("tx proposed");
            }
            Ok(Some(tx_with_status)) if tx_with_status.tx_status.status == Status::Committed => {
                log::info!("tx commited");
                return Ok(tx_with_status.transaction);
            }
            res => {
                log::error!("unexpected response of get_transaction: {:?}", res)
            }
        }
    }
    Err(anyhow!("Timeout: {:?}", retry_timeout))
}

pub fn get_network_type(rpc_client: &mut HttpRpcClient) -> Result<NetworkType> {
    let chain_info = rpc_client
        .get_blockchain_info()
        .map_err(|err| anyhow!(err))?;
    NetworkType::from_raw_str(chain_info.chain.as_str())
        .ok_or_else(|| anyhow!("Unexpected network type: {}", chain_info.chain))
}

pub fn run_in_output_mode<I, S>(bin: &str, args: I) -> Result<(String, String)>
where
    I: IntoIterator<Item = S> + std::fmt::Debug,
    S: AsRef<OsStr>,
{
    log::debug!("[Execute]: {} {:?}", bin, args);
    let init_output = Command::new(bin.to_owned())
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
    let config = toml::from_slice(&content)?;
    Ok(config)
}

pub fn wait_for_l2_tx(
    godwoken_rpc_client: &mut GodwokenRpcClient,
    tx_hash: &H256,
    timeout_secs: u64,
    quiet: bool,
) -> Result<Option<TxReceipt>> {
    let retry_timeout = Duration::from_secs(timeout_secs);
    let start_time = Instant::now();
    while start_time.elapsed() < retry_timeout {
        std::thread::sleep(Duration::from_secs(2));

        let receipt = godwoken_rpc_client.get_transaction_receipt(tx_hash)?;

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
