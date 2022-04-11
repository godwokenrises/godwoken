use crate::{
    types::{BuildScriptsResult, DeployItem, Programs, ScriptsDeploymentResult},
    utils::transaction::{get_network_type, run_cmd, wait_for_tx, TYPE_ID_CODE_HASH},
};
use anyhow::{anyhow, Result};
use ckb_fixed_hash::H256;
use ckb_jsonrpc_types::{CellDep, DepType, OutPoint, Script};
use ckb_sdk::{Address, AddressPayload, HttpRpcClient, HumanCapacity};
use ckb_types::{
    bytes::Bytes,
    core::{Capacity, ScriptHashType},
    packed,
    prelude::*,
};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use std::str::FromStr;

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug, Default)]
struct DeploymentIndex {
    pub programs: Programs,
    pub lock: Script,
}

pub fn deploy_program(
    privkey_path: &Path,
    rpc_client: &mut HttpRpcClient,
    binary_path: &Path,
    target_lock: &packed::Script,
    target_address: &Address,
) -> Result<DeployItem> {
    log::info!("deploy binary {:?}", binary_path);
    let file_size = fs::metadata(binary_path)?.len();
    let min_output_capacity = {
        let data_capacity = Capacity::bytes(file_size as usize)?;
        let type_script = packed::Script::new_builder()
            .code_hash(TYPE_ID_CODE_HASH.pack())
            .hash_type(ScriptHashType::Type.into())
            .args(Bytes::from(vec![0u8; 32]).pack())
            .build();
        let output = packed::CellOutput::new_builder()
            .lock(target_lock.clone())
            .type_(Some(type_script).pack())
            .build();
        output.occupied_capacity(data_capacity)?.as_u64()
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
    let tx_hash = H256::from_str(output.trim().trim_start_matches("0x"))?;
    log::info!("tx_hash: {:#x}", tx_hash);

    let tx = wait_for_tx(rpc_client, &tx_hash, 300)?;
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
    scripts_result: &BuildScriptsResult,
) -> Result<ScriptsDeploymentResult> {
    if let Err(err) = run_cmd(vec!["--version"]) {
        return Err(anyhow!(
            "Please install ckb-cli (cargo install ckb-cli) first: {}",
            err
        ));
    }

    let mut rpc_client = HttpRpcClient::new(ckb_rpc_url.to_string());
    let network_type = get_network_type(&mut rpc_client)?;
    let target_lock = packed::Script::from(scripts_result.lock.clone());
    let address_payload = AddressPayload::from(target_lock.clone());
    let target_address = Address::new(network_type, address_payload);

    let mut total_file_size = 0;
    for path in &[
        &scripts_result.programs.custodian_lock,
        &scripts_result.programs.deposit_lock,
        &scripts_result.programs.withdrawal_lock,
        &scripts_result.programs.challenge_lock,
        &scripts_result.programs.stake_lock,
        &scripts_result.programs.omni_lock,
        &scripts_result.programs.state_validator,
        &scripts_result.programs.l2_sudt_validator,
        &scripts_result.programs.eth_account_lock,
        &scripts_result.programs.meta_contract_validator,
        &scripts_result.programs.polyjuice_validator,
        &scripts_result.programs.eth_addr_reg_validator,
    ] {
        match fs::metadata(path).map_err(|err| err.to_string()) {
            Ok(metadata) => {
                if !metadata.is_file() {
                    return Err(anyhow!("binary path is not a file: {:?}", path));
                }
                total_file_size += metadata.len();
                log::info!("cost {:>6} CKBytes for file: {:?}", metadata.len(), path);
            }
            Err(err) => {
                return Err(anyhow!("error read metadata of {:?}, error: {}", path, err));
            }
        }
    }
    log::info!("total_file_size: {}", total_file_size);

    let custodian_lock = deploy_program(
        privkey_path,
        &mut rpc_client,
        &scripts_result.programs.custodian_lock,
        &target_lock,
        &target_address,
    )?;
    let deposit_lock = deploy_program(
        privkey_path,
        &mut rpc_client,
        &scripts_result.programs.deposit_lock,
        &target_lock,
        &target_address,
    )?;
    let withdrawal_lock = deploy_program(
        privkey_path,
        &mut rpc_client,
        &scripts_result.programs.withdrawal_lock,
        &target_lock,
        &target_address,
    )?;
    let challenge_lock = deploy_program(
        privkey_path,
        &mut rpc_client,
        &scripts_result.programs.challenge_lock,
        &target_lock,
        &target_address,
    )?;
    let stake_lock = deploy_program(
        privkey_path,
        &mut rpc_client,
        &scripts_result.programs.stake_lock,
        &target_lock,
        &target_address,
    )?;
    let omni_lock = deploy_program(
        privkey_path,
        &mut rpc_client,
        &scripts_result.programs.omni_lock,
        &target_lock,
        &target_address,
    )?;
    let state_validator = deploy_program(
        privkey_path,
        &mut rpc_client,
        &scripts_result.programs.state_validator,
        &target_lock,
        &target_address,
    )?;
    let l2_sudt_validator = deploy_program(
        privkey_path,
        &mut rpc_client,
        &scripts_result.programs.l2_sudt_validator,
        &target_lock,
        &target_address,
    )?;
    let meta_contract_validator = deploy_program(
        privkey_path,
        &mut rpc_client,
        &scripts_result.programs.meta_contract_validator,
        &target_lock,
        &target_address,
    )?;
    let eth_account_lock = deploy_program(
        privkey_path,
        &mut rpc_client,
        &scripts_result.programs.eth_account_lock,
        &target_lock,
        &target_address,
    )?;
    // FIXME: write godwoken-polyjuice binary to named temp file then use the path
    let polyjuice_validator = deploy_program(
        privkey_path,
        &mut rpc_client,
        &scripts_result.programs.polyjuice_validator,
        &target_lock,
        &target_address,
    )?;
    let eth_addr_reg_validator = deploy_program(
        privkey_path,
        &mut rpc_client,
        &scripts_result.programs.eth_addr_reg_validator,
        &target_lock,
        &target_address,
    )?;
    let deployment_result = ScriptsDeploymentResult {
        custodian_lock,
        deposit_lock,
        withdrawal_lock,
        challenge_lock,
        stake_lock,
        omni_lock,
        state_validator,
        l2_sudt_validator,
        meta_contract_validator,
        eth_account_lock,
        polyjuice_validator,
        eth_addr_reg_validator,
    };
    Ok(deployment_result)
}
