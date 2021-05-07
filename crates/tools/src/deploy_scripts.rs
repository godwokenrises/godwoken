use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::str::FromStr;
use std::time::{Duration, Instant};

use ckb_fixed_hash::{h256, H256};
use ckb_jsonrpc_types::{CellDep, DepType, OutPoint, Script, Status};
use ckb_sdk::{
    rpc::TransactionView, Address, AddressPayload, HttpRpcClient, HumanCapacity, NetworkType,
};
use ckb_types::{
    bytes::Bytes,
    core::{Capacity, ScriptHashType},
    packed,
    prelude::*,
};
use serde::{Deserialize, Serialize};

// "TYPE_ID" in hex
pub const TYPE_ID_CODE_HASH: H256 = h256!("0x545950455f4944");

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug, Default)]
pub struct Programs {
    // path: godwoken-scripts/build/release/custodian-lock
    pub custodian_lock: PathBuf,
    // path: godwoken-scripts/build/release/deposition-lock
    pub deposition_lock: PathBuf,
    // path: godwoken-scripts/build/release/withdrawal-lock
    pub withdrawal_lock: PathBuf,
    // path: godwoken-scripts/build/release/challenge-lock
    pub challenge_lock: PathBuf,
    // path: godwoken-scripts/build/release/stake-lock
    pub stake_lock: PathBuf,
    // path: godwoken-scripts/build/release/state-validator
    pub state_validator: PathBuf,
    // path: godwoken-scripts/c/build/sudt-validator
    pub l2_sudt_validator: PathBuf,

    // path: godwoken-scripts/c/build/account_locks/eth-account-lock
    pub eth_account_lock: PathBuf,
    // path: godwoken-scripts/c/build/account_locks/tron-account-lock
    pub tron_account_lock: PathBuf,

    // path: godwoken-scripts/c/build/meta-contract-validator
    pub meta_contract_validator: PathBuf,
    // path: godwoken-polyjuice/build/validator
    pub polyjuice_validator: PathBuf,

    // path: clerkb/build/debug/poa.strip
    pub state_validator_lock: PathBuf,
    // path: clerkb/build/debug/state.strip
    pub poa_state: PathBuf,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug, Default)]
pub struct DeploymentIndex {
    pub programs: Programs,
    pub lock: Script,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug, Default)]
pub struct DeployItem {
    pub script_type_hash: H256,
    pub cell_dep: CellDep,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug, Default)]
pub struct ScriptsDeploymentResult {
    pub custodian_lock: DeployItem,
    pub deposition_lock: DeployItem,
    pub withdrawal_lock: DeployItem,
    pub challenge_lock: DeployItem,
    pub stake_lock: DeployItem,
    pub state_validator: DeployItem,
    pub meta_contract_validator: DeployItem,
    pub l2_sudt_validator: DeployItem,
    pub eth_account_lock: DeployItem,
    pub tron_account_lock: DeployItem,
    pub polyjuice_validator: DeployItem,
    pub state_validator_lock: DeployItem,
    pub poa_state: DeployItem,
}

pub fn get_network_type(rpc_client: &mut HttpRpcClient) -> Result<NetworkType, String> {
    let chain_info = rpc_client.get_blockchain_info()?;
    NetworkType::from_raw_str(chain_info.chain.as_str())
        .ok_or_else(|| format!("Unexpected network type: {}", chain_info.chain))
}

pub fn run_cmd<I, S>(args: I) -> Result<String, String>
where
    I: IntoIterator<Item = S> + std::fmt::Debug,
    S: AsRef<OsStr>,
{
    let bin = "ckb-cli";
    log::info!("[Execute]: {} {:?}", bin, args);
    let init_output = Command::new(bin.to_owned())
        .env("RUST_BACKTRACE", "full")
        .args(args)
        .output()
        .expect("Run command failed");

    if !init_output.status.success() {
        Err(format!(
            "{}",
            String::from_utf8_lossy(init_output.stderr.as_slice())
        ))
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
) -> Result<TransactionView, String> {
    let retry_timeout = Duration::from_secs(timeout_secs);
    let start_time = Instant::now();
    while start_time.elapsed() < retry_timeout {
        std::thread::sleep(Duration::from_secs(2));
        match rpc_client.get_transaction(tx_hash.clone())? {
            Some(tx_with_status) if tx_with_status.tx_status.status == Status::Pending => {
                log::info!("tx pending");
            }
            Some(tx_with_status) if tx_with_status.tx_status.status == Status::Proposed => {
                log::info!("tx proposed");
            }
            Some(tx_with_status) if tx_with_status.tx_status.status == Status::Committed => {
                log::info!("tx commited");
                return Ok(tx_with_status.transaction);
            }
            _ => {
                log::error!("error")
            }
        }
    }
    Err(format!("Timeout: {:?}", retry_timeout))
}

pub fn deploy_program(
    privkey_path: &Path,
    rpc_client: &mut HttpRpcClient,
    binary_path: &Path,
    target_lock: &packed::Script,
    target_address: &Address,
) -> Result<DeployItem, String> {
    log::info!("deploy binary {:?}", binary_path);
    let file_size = fs::metadata(binary_path)
        .map_err(|err| err.to_string())?
        .len();
    let min_output_capacity = {
        let data_capacity = Capacity::bytes(file_size as usize).map_err(|err| err.to_string())?;
        let type_script = packed::Script::new_builder()
            .code_hash(TYPE_ID_CODE_HASH.pack())
            .hash_type(ScriptHashType::Type.into())
            .args(Bytes::from(vec![0u8; 32]).pack())
            .build();
        let output = packed::CellOutput::new_builder()
            .lock(target_lock.clone())
            .type_(Some(type_script).pack())
            .build();
        output
            .occupied_capacity(data_capacity)
            .map_err(|err| err.to_string())?
            .as_u64()
    };
    let capacity_string = HumanCapacity(min_output_capacity).to_string();
    let target_address_string = target_address.to_string();
    let tx_fee_str = "0.1";

    /* ckb-cli
        --url {ckb_rpc_url}
        wallet transfer
        --privkey-path {privkey_path}
        --to-address {target_address}
        --to-data-path {binary_path}
        --capacity {capacity?}
        --tx-fee {fee?}
        --type-id
        --skip-check-to-address
    */
    log::info!(
        "file_size: {} bytes, output cell capacity: {} CKB",
        file_size,
        capacity_string
    );
    let output = run_cmd(vec![
        "--url",
        rpc_client.url(),
        "wallet",
        "transfer",
        "--privkey-path",
        privkey_path.to_str().expect("non-utf8 file path"),
        "--to-address",
        target_address_string.as_str(),
        "--to-data-path",
        binary_path.to_str().expect("non-utf8 file path"),
        "--capacity",
        capacity_string.as_str(),
        "--tx-fee",
        tx_fee_str,
        "--type-id",
        "--skip-check-to-address",
    ])?;
    let tx_hash = H256::from_str(&output.trim()[2..]).map_err(|err| err.to_string())?;
    log::info!("tx_hash: {:#x}", tx_hash);

    let tx = wait_for_tx(rpc_client, &tx_hash, 120)?;
    let first_output_type_script = tx.inner.outputs[0].type_.clone().expect("type id cell");
    let script_type_hash: H256 = packed::Script::from(first_output_type_script)
        .calc_script_hash()
        .unpack();
    let cell_dep = CellDep {
        out_point: OutPoint {
            tx_hash,
            index: 0.into(),
        },
        dep_type: DepType::Code,
    };
    Ok(DeployItem {
        script_type_hash,
        cell_dep,
    })
}

pub fn deploy_scripts(
    privkey_path: &Path,
    ckb_rpc_url: &str,
    input_path: &Path,
    output_path: &Path,
) -> Result<(), String> {
    if let Err(err) = run_cmd(vec!["--version"]) {
        return Err(format!(
            "Please install ckb-cli (cargo install ckb-cli) first: {}",
            err
        ));
    }

    let input = fs::read_to_string(input_path).map_err(|err| err.to_string())?;
    let deployment_index: DeploymentIndex =
        serde_json::from_str(input.as_str()).map_err(|err| err.to_string())?;

    let mut rpc_client = HttpRpcClient::new(ckb_rpc_url.to_string());
    let network_type = get_network_type(&mut rpc_client)?;
    let target_lock = packed::Script::from(deployment_index.lock);
    let address_payload = AddressPayload::from(target_lock.clone());
    let target_address = Address::new(network_type, address_payload);

    let mut total_file_size = 0;
    for path in &[
        &deployment_index.programs.custodian_lock,
        &deployment_index.programs.deposition_lock,
        &deployment_index.programs.withdrawal_lock,
        &deployment_index.programs.challenge_lock,
        &deployment_index.programs.stake_lock,
        &deployment_index.programs.state_validator,
        &deployment_index.programs.l2_sudt_validator,
        &deployment_index.programs.eth_account_lock,
        &deployment_index.programs.tron_account_lock,
        &deployment_index.programs.meta_contract_validator,
        &deployment_index.programs.polyjuice_validator,
        &deployment_index.programs.state_validator_lock,
        &deployment_index.programs.poa_state,
    ] {
        match fs::metadata(path).map_err(|err| err.to_string()) {
            Ok(metadata) => {
                if !metadata.is_file() {
                    return Err(format!("binary path is not a file: {:?}", path));
                }
                total_file_size += metadata.len();
                log::info!("cost {:>6} CKBytes for file: {:?}", metadata.len(), path);
            }
            Err(err) => {
                return Err(format!("error read metadata of {:?}, error: {}", path, err));
            }
        }
    }
    log::info!("total_file_size: {}", total_file_size);

    let custodian_lock = deploy_program(
        privkey_path,
        &mut rpc_client,
        &deployment_index.programs.custodian_lock,
        &target_lock,
        &target_address,
    )?;
    let deposition_lock = deploy_program(
        privkey_path,
        &mut rpc_client,
        &deployment_index.programs.deposition_lock,
        &target_lock,
        &target_address,
    )?;
    let withdrawal_lock = deploy_program(
        privkey_path,
        &mut rpc_client,
        &deployment_index.programs.withdrawal_lock,
        &target_lock,
        &target_address,
    )?;
    let challenge_lock = deploy_program(
        privkey_path,
        &mut rpc_client,
        &deployment_index.programs.challenge_lock,
        &target_lock,
        &target_address,
    )?;
    let stake_lock = deploy_program(
        privkey_path,
        &mut rpc_client,
        &deployment_index.programs.stake_lock,
        &target_lock,
        &target_address,
    )?;
    let state_validator = deploy_program(
        privkey_path,
        &mut rpc_client,
        &deployment_index.programs.state_validator,
        &target_lock,
        &target_address,
    )?;
    let l2_sudt_validator = deploy_program(
        privkey_path,
        &mut rpc_client,
        &deployment_index.programs.l2_sudt_validator,
        &target_lock,
        &target_address,
    )?;
    let meta_contract_validator = deploy_program(
        privkey_path,
        &mut rpc_client,
        &deployment_index.programs.meta_contract_validator,
        &target_lock,
        &target_address,
    )?;
    let eth_account_lock = deploy_program(
        privkey_path,
        &mut rpc_client,
        &deployment_index.programs.eth_account_lock,
        &target_lock,
        &target_address,
    )?;
    let tron_account_lock = deploy_program(
        privkey_path,
        &mut rpc_client,
        &deployment_index.programs.tron_account_lock,
        &target_lock,
        &target_address,
    )?;
    // FIXME: write godwoken-polyjuice binary to named temp file then use the path
    let polyjuice_validator = deploy_program(
        privkey_path,
        &mut rpc_client,
        &deployment_index.programs.polyjuice_validator,
        &target_lock,
        &target_address,
    )?;
    let state_validator_lock = deploy_program(
        privkey_path,
        &mut rpc_client,
        &deployment_index.programs.state_validator_lock,
        &target_lock,
        &target_address,
    )?;
    let poa_state = deploy_program(
        privkey_path,
        &mut rpc_client,
        &deployment_index.programs.poa_state,
        &target_lock,
        &target_address,
    )?;
    let deployment_result = ScriptsDeploymentResult {
        custodian_lock,
        deposition_lock,
        withdrawal_lock,
        challenge_lock,
        stake_lock,
        state_validator,
        l2_sudt_validator,
        meta_contract_validator,
        eth_account_lock,
        tron_account_lock,
        polyjuice_validator,
        state_validator_lock,
        poa_state,
    };
    let output_content =
        serde_json::to_string_pretty(&deployment_result).expect("serde json to string pretty");
    fs::write(output_path, output_content.as_bytes()).map_err(|err| err.to_string())?;
    Ok(())
}
