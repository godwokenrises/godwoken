use anyhow::{anyhow, Result};
use ckb_fixed_hash::H256;
use ckb_jsonrpc_types::JsonBytes;
use ckb_types::prelude::{Builder, Entity};
use gw_types::packed::{L2Transaction, RawL2Transaction};
use std::path::Path;

use crate::{
    account::{
        eth_sign, l2_script_hash_to_short_address, parse_account_short_address,
        privkey_to_l2_script_hash, read_privkey, short_address_to_account_id,
    },
    godwoken_rpc::GodwokenRpcClient,
    types::ScriptsDeploymentResult,
    utils::{
        message::generate_transaction_message_to_sign,
        transaction::{read_config, wait_for_l2_tx},
    },
};
use gw_types::{bytes::Bytes as GwBytes, prelude::Pack as GwPack};

const GW_LOG_POLYJUICE_SYSTEM: u8 = 0x2;

#[allow(clippy::too_many_arguments)]
pub async fn deploy(
    godwoken_rpc_url: &str,
    config_path: &Path,
    scripts_deployment_path: &Path,
    privkey_path: &Path,
    creator_account_id: u32,
    gas_limit: u64,
    gas_price: u128,
    data: &str,
    value: u128,
) -> Result<()> {
    let data = GwBytes::from(hex::decode(data.trim_start_matches("0x").as_bytes())?);

    let scripts_deployment_string = std::fs::read_to_string(scripts_deployment_path)?;
    let scripts_deployment: ScriptsDeploymentResult =
        serde_json::from_str(&scripts_deployment_string)?;

    let config = read_config(config_path)?;
    let rollup_type_hash = &config.genesis.rollup_type_hash;

    let mut godwoken_rpc_client = GodwokenRpcClient::new(godwoken_rpc_url);

    let privkey = read_privkey(privkey_path)?;

    send(
        &mut godwoken_rpc_client,
        GwBytes::from(Vec::<u8>::new()),
        creator_account_id,
        &privkey,
        gas_limit,
        gas_price,
        data,
        value,
        rollup_type_hash,
        &scripts_deployment,
    )
    .await?;

    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn send_transaction(
    godwoken_rpc_url: &str,
    config_path: &Path,
    scripts_deployment_path: &Path,
    privkey_path: &Path,
    creator_account_id: u32,
    gas_limit: u64,
    gas_price: u128,
    data: &str,
    value: u128,
    to_address: &str,
) -> Result<()> {
    let data = GwBytes::from(hex::decode(data.trim_start_matches("0x").as_bytes())?);

    let scripts_deployment_string = std::fs::read_to_string(scripts_deployment_path)?;
    let scripts_deployment: ScriptsDeploymentResult =
        serde_json::from_str(&scripts_deployment_string)?;

    let config = read_config(config_path)?;
    let rollup_type_hash = &config.genesis.rollup_type_hash;

    let mut godwoken_rpc_client = GodwokenRpcClient::new(godwoken_rpc_url);

    let privkey = read_privkey(privkey_path)?;

    let to_address = GwBytes::from(hex::decode(to_address.trim_start_matches("0x").as_bytes())?);

    send(
        &mut godwoken_rpc_client,
        to_address,
        creator_account_id,
        &privkey,
        gas_limit,
        gas_price,
        data,
        value,
        rollup_type_hash,
        &scripts_deployment,
    )
    .await?;

    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn polyjuice_call(
    godwoken_rpc_url: &str,
    gas_limit: u64,
    gas_price: u128,
    data: &str,
    value: u128,
    to_address: &str,
    from: &str,
) -> Result<()> {
    let data = GwBytes::from(hex::decode(data.trim_start_matches("0x").as_bytes())?);

    let mut godwoken_rpc_client = GodwokenRpcClient::new(godwoken_rpc_url);

    let to_address_str = to_address;
    let to_address = GwBytes::from(hex::decode(
        to_address_str.trim_start_matches("0x").as_bytes(),
    )?);

    let from_address = parse_account_short_address(&mut godwoken_rpc_client, from).await?;
    let from_id = short_address_to_account_id(&mut godwoken_rpc_client, &from_address).await?;
    let from_id = from_id.expect("from account not found!");
    let nonce = godwoken_rpc_client.get_nonce(from_id).await?;

    let to_script_hash = match godwoken_rpc_client
        .get_script_hash_by_short_address(JsonBytes::from_bytes(to_address))
        .await?
    {
        Some(h) => h,
        None => {
            return Err(anyhow!(
                "script hash by short address {} not found",
                to_address_str
            ))
        }
    };
    let to_id = godwoken_rpc_client
        .get_account_id_by_script_hash(to_script_hash)
        .await?;
    let to_id = to_id.expect("to id not found!");

    let creator_account_id = 0u32;
    let args = encode_polyjuice_args(gas_limit, gas_price, value, data, to_id, creator_account_id);
    let real_to_id = if to_id > 0 { to_id } else { creator_account_id };

    let raw_l2transaction = RawL2Transaction::new_builder()
        .from_id(from_id.pack())
        .to_id(real_to_id.pack())
        .nonce(nonce.pack())
        .args(args.pack())
        .build();

    log::info!("raw l2 transaction: {}", raw_l2transaction);

    let run_result = godwoken_rpc_client
        .execute_raw_l2transaction(JsonBytes::from_bytes(raw_l2transaction.as_bytes()))
        .await?;

    let j = serde_json::to_value(run_result)?;
    log::info!("run result: {}", serde_json::to_string_pretty(&j).unwrap());

    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn send(
    godwoken_rpc_client: &mut GodwokenRpcClient,
    to_address: GwBytes,
    creator_account_id: u32,
    privkey: &H256,
    gas_limit: u64,
    gas_price: u128,
    data: GwBytes,
    value: u128,
    rollup_type_hash: &H256,
    scripts_deployment: &ScriptsDeploymentResult,
) -> Result<()> {
    let to_address = if to_address == [0u8; 20][..] || to_address.is_empty() {
        None
    } else {
        Some(to_address)
    };

    let l2_script_hash = privkey_to_l2_script_hash(privkey, rollup_type_hash, scripts_deployment)?;
    let from_address = l2_script_hash_to_short_address(&l2_script_hash);
    let from_id = short_address_to_account_id(godwoken_rpc_client, &from_address)
        .await?
        .expect("Can find account id by privkey!");

    let nonce = godwoken_rpc_client.get_nonce(from_id).await?;

    let to_id = match to_address.clone() {
        None => creator_account_id,
        Some(addr) => {
            let script_hash = godwoken_rpc_client
                .get_script_hash_by_short_address(JsonBytes::from_bytes(addr))
                .await?;
            let script_hash = script_hash.expect("to script_hash not found!");
            let id = godwoken_rpc_client
                .get_account_id_by_script_hash(script_hash)
                .await?;

            id.expect("to id not found!")
        }
    };

    let args = encode_polyjuice_args(gas_limit, gas_price, value, data, to_id, creator_account_id);
    let real_to_id = if to_id > 0 { to_id } else { creator_account_id };

    let raw_l2_transaction = RawL2Transaction::new_builder()
        .from_id(from_id.pack())
        .to_id(real_to_id.pack())
        .nonce(nonce.pack())
        .args(args.pack())
        .build();

    let sender_script_hash = godwoken_rpc_client.get_script_hash(from_id).await?;
    let receiver_script_hash = godwoken_rpc_client.get_script_hash(to_id).await?;
    let message = generate_transaction_message_to_sign(
        &raw_l2_transaction,
        rollup_type_hash,
        &sender_script_hash,
        &receiver_script_hash,
    );

    let signature = eth_sign(&message, privkey.clone())?;
    let l2_tx = L2Transaction::new_builder()
        .raw(raw_l2_transaction)
        .signature(signature.pack())
        .build();

    let tx_hash = godwoken_rpc_client
        .submit_l2transaction(JsonBytes::from_bytes(l2_tx.as_bytes()))
        .await?;
    log::info!("tx hash: 0x{}", hex::encode(tx_hash.as_bytes()));

    let tx_receipt = wait_for_l2_tx(godwoken_rpc_client, &tx_hash, 180, false)?;

    if let (None, Some(receipt)) = (to_address, tx_receipt) {
        let polyjuice_system_log = receipt
            .logs
            .into_iter()
            .find(|item| item.service_flag.value() as u8 == GW_LOG_POLYJUICE_SYSTEM)
            .ok_or_else(|| anyhow!("no system logs"))?;
        let data = polyjuice_system_log.data.as_bytes();
        let mut contract_address = [0u8; 20];
        contract_address.copy_from_slice(&data[16..36]);
        log::info!("contract address: 0x{}", hex::encode(contract_address));
    };

    Ok(())
}

fn encode_polyjuice_args(
    gas_limit: u64,
    gas_price: u128,
    value: u128,
    data: GwBytes,
    to_id: u32,
    creator_account_id: u32,
) -> GwBytes {
    let mut args = vec![0u8; 52];
    args[0..7].copy_from_slice(b"\xFF\xFF\xFFPOLY");
    args[7] = if to_id == 0 || to_id == creator_account_id {
        3
    } else {
        0
    };
    args[8..16].copy_from_slice(&gas_limit.to_le_bytes());
    args[16..32].copy_from_slice(&gas_price.to_le_bytes());
    args[32..48].copy_from_slice(&value.to_le_bytes());
    let data_length = data.len() as u32;
    args[48..52].copy_from_slice(&data_length.to_le_bytes());
    args.append(&mut data.to_vec());

    GwBytes::from(args)
}
