use ckb_jsonrpc_types::JsonBytes;
use ckb_types::prelude::{Builder, Entity};
use gw_types::{
    core::ScriptHashType,
    packed::{CreateAccount, Fee, L2Transaction, MetaContractArgs, RawL2Transaction, Script},
};
use std::path::Path;

use crate::{
    account::{eth_sign, privkey_to_short_address, read_privkey, short_address_to_account_id},
    deploy_scripts::ScriptsDeploymentResult,
    godwoken_rpc::GodwokenRpcClient,
    transfer::generate_transaction_message_to_sign,
    utils::transaction::{read_config, wait_for_l2_tx},
};
use gw_types::{bytes::Bytes as GwBytes, prelude::Pack as GwPack};

pub fn create_creator_account(
    godwoken_rpc_url: &str,
    privkey_path: &Path,
    sudt_id: u32,
    fee_amount: &str,
    config_path: &Path,
    deployment_results_path: &Path,
) -> Result<(), String> {
    let fee: u128 = fee_amount.parse().expect("fee format error");

    let deployment_result_string =
        std::fs::read_to_string(deployment_results_path).map_err(|err| err.to_string())?;
    let deployment_result: ScriptsDeploymentResult =
        serde_json::from_str(&deployment_result_string).map_err(|err| err.to_string())?;

    let mut godwoken_rpc_client = GodwokenRpcClient::new(godwoken_rpc_url);

    let config = read_config(config_path)?;
    let rollup_type_hash = &config.genesis.rollup_type_hash;

    let privkey = read_privkey(privkey_path)?;
    let from_address = privkey_to_short_address(&privkey, rollup_type_hash, &deployment_result)?;
    let from_id = short_address_to_account_id(&mut godwoken_rpc_client, &from_address)?;
    let from_id = from_id.expect("Account id of provided privkey not found!");
    log::info!("from id: {}", from_id);

    let nonce = godwoken_rpc_client.get_nonce(from_id)?;

    let validator_script_hash = &config.backends[2].validator_script_type_hash;

    let mut l2_args_vec = rollup_type_hash.as_bytes().to_vec();
    l2_args_vec.append(&mut sudt_id.to_le_bytes().to_vec());
    let l2_script_args = GwPack::pack(&GwBytes::from(l2_args_vec));
    let l2_script = Script::new_builder()
        .code_hash(validator_script_hash.pack())
        .hash_type(ScriptHashType::Type.into())
        .args(l2_script_args)
        .build();
    let l2_script_hash = l2_script.hash();
    log::info!("l2 script hash: 0x{}", hex::encode(l2_script_hash));

    let account_id = godwoken_rpc_client.get_account_id_by_script_hash(l2_script_hash.into())?;
    if let Some(id) = account_id {
        log::info!("Creator account id already exists: {}", id);
        return Ok(());
    }

    let fee = Fee::new_builder()
        .sudt_id(sudt_id.pack())
        .amount(fee.pack())
        .build();

    let create_account = CreateAccount::new_builder()
        .script(l2_script)
        .fee(fee)
        .build();

    let args = MetaContractArgs::new_builder().set(create_account).build();

    let account_raw_l2_transaction = RawL2Transaction::new_builder()
        .from_id(from_id.pack())
        .to_id(0u32.pack())
        .nonce(nonce.pack())
        .args(args.as_bytes().pack())
        .build();

    let sender_script_hash = godwoken_rpc_client.get_script_hash(from_id)?;
    let receiver_script_hash = godwoken_rpc_client.get_script_hash(0)?;

    let message = generate_transaction_message_to_sign(
        &account_raw_l2_transaction,
        rollup_type_hash,
        &sender_script_hash,
        &receiver_script_hash,
    );

    let signature = eth_sign(&message, privkey)?;
    let account_l2_transaction = L2Transaction::new_builder()
        .raw(account_raw_l2_transaction)
        .signature(signature.pack())
        .build();

    let json_bytes = JsonBytes::from_bytes(account_l2_transaction.as_bytes());
    let tx_hash = godwoken_rpc_client.submit_l2transaction(json_bytes)?;
    log::info!("tx hash: 0x{}", hex::encode(tx_hash.as_bytes()));

    wait_for_l2_tx(&mut godwoken_rpc_client, &tx_hash, 180)?;

    let account_id = godwoken_rpc_client
        .get_account_id_by_script_hash(l2_script_hash.into())?
        .expect("Creator account id not exist!");
    log::info!("Creator account id: {}", account_id);

    Ok(())
}
