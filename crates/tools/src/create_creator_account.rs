use anyhow::{anyhow, Result};
use ckb_jsonrpc_types::JsonBytes;
use ckb_types::prelude::{Builder, Entity};
use gw_common::builtins::RESERVED_ACCOUNT_ID;
use gw_config::{BackendType, Config};
use gw_types::{
    core::ScriptHashType,
    packed::{CreateAccount, L2Transaction, MetaContractArgs, RawL2Transaction, Script},
};
use std::path::Path;

use crate::{
    account::{
        eth_sign, privkey_to_short_script_hash, read_privkey, short_script_hash_to_account_id,
    },
    godwoken_rpc::GodwokenRpcClient,
    types::ScriptsDeploymentResult,
    utils::{
        message::generate_transaction_message_to_sign,
        transaction::{read_config, wait_for_l2_tx},
    },
};
use gw_types::{bytes::Bytes as GwBytes, prelude::Pack as GwPack};

/// create ETH Address Registry account
async fn create_eth_addr_reg_account(
    godwoken_rpc_client: &mut GodwokenRpcClient,
    privkey: &ckb_fixed_hash::H256,
    fee_amount: u64,
    config: &Config,
    scripts_deploy_result: &ScriptsDeploymentResult,
) -> Result<u32> {
    let rollup_type_hash = &config.genesis.rollup_type_hash;
    let from_address =
        privkey_to_short_script_hash(privkey, rollup_type_hash, scripts_deploy_result)?;
    let from_id = short_script_hash_to_account_id(godwoken_rpc_client, &from_address).await?;
    let from_id = from_id.expect("Account id of provided privkey not found!");

    let eth_addr_reg_validator_script_hash = {
        let mut backends = config.backends.iter();
        let eth_addr_reg_backend = backends
            .find(|backend| backend.backend_type == BackendType::EthAddrReg)
            .ok_or_else(|| anyhow!("EthAddrReg backend not found in config"))?;
        &eth_addr_reg_backend.validator_script_type_hash
    };
    let l2_args_vec = rollup_type_hash.as_bytes().to_vec();
    let l2_script_args = GwPack::pack(&GwBytes::from(l2_args_vec));
    let eth_addr_reg_script = Script::new_builder()
        .code_hash(eth_addr_reg_validator_script_hash.pack())
        .hash_type(ScriptHashType::Type.into())
        .args(l2_script_args)
        .build();
    let eth_addr_reg_script_hash = eth_addr_reg_script.hash();
    log::info!(
        "eth_addr_reg_script_hash: 0x{}",
        hex::encode(eth_addr_reg_script_hash)
    );

    let eth_addr_reg_id =
        godwoken_rpc_client.get_account_id_by_script_hash(eth_addr_reg_script_hash.into()).await?;
    if let Some(id) = eth_addr_reg_id {
        log::info!("ETH Address Registry account already exists, id = {}", id);
        return Ok(id);
    }

    let nonce = godwoken_rpc_client.get_nonce(from_id).await?;
    let create_account = CreateAccount::new_builder()
        .script(eth_addr_reg_script)
        .fee(fee_amount.pack())
        .build();
    let l2tx_args = MetaContractArgs::new_builder().set(create_account).build();
    let raw_l2tx = RawL2Transaction::new_builder()
        .from_id(from_id.pack())
        .to_id(RESERVED_ACCOUNT_ID.pack())
        .nonce(nonce.pack())
        .args(l2tx_args.as_bytes().pack())
        .build();

    let sender_script_hash = godwoken_rpc_client.get_script_hash(from_id).await?;
    let receiver_script_hash = godwoken_rpc_client.get_script_hash(RESERVED_ACCOUNT_ID).await?;

    let message = generate_transaction_message_to_sign(
        &raw_l2tx,
        rollup_type_hash,
        &sender_script_hash,
        &receiver_script_hash,
    );
    let signature = eth_sign(&message, privkey.to_owned())?;
    let l2tx = L2Transaction::new_builder()
        .raw(raw_l2tx)
        .signature(signature.pack())
        .build();

    let json_bytes = JsonBytes::from_bytes(l2tx.as_bytes());
    let tx_hash = godwoken_rpc_client.submit_l2transaction(json_bytes).await?;
    log::info!("tx hash: 0x{}", hex::encode(tx_hash.as_bytes()));
    wait_for_l2_tx(godwoken_rpc_client, &tx_hash, 180, false)?;

    let eth_addr_reg_id = godwoken_rpc_client
        .get_account_id_by_script_hash(eth_addr_reg_script_hash.into()).await?
        .expect("ETH Address Registry account id");
    log::info!("ETH Address Registry account id: {}", eth_addr_reg_id);

    Ok(eth_addr_reg_id)
}

pub async fn create_creator_account(
    godwoken_rpc_url: &str,
    privkey_path: &Path,
    sudt_id: u32,
    fee_amount: &str,
    config_path: &Path,
    scripts_deployment_path: &Path,
) -> Result<()> {
    let fee: u64 = fee_amount.parse().expect("fee format error");

    let scripts_deployment_content = std::fs::read_to_string(scripts_deployment_path)?;
    let scripts_deployment: ScriptsDeploymentResult =
        serde_json::from_str(&scripts_deployment_content)?;

    let mut godwoken_rpc_client = GodwokenRpcClient::new(godwoken_rpc_url);

    let config = read_config(config_path)?;
    let rollup_type_hash = &config.genesis.rollup_type_hash;

    let privkey = read_privkey(privkey_path)?;

    let eth_registry_id = create_eth_addr_reg_account(
        &mut godwoken_rpc_client,
        &privkey,
        fee,
        &config,
        &scripts_deployment,
    ).await
    .expect("create_eth_addr_reg_account success");

    let from_address =
        privkey_to_short_script_hash(&privkey, rollup_type_hash, &scripts_deployment)?;
    let from_id = short_script_hash_to_account_id(&mut godwoken_rpc_client, &from_address).await?;
    let from_id = from_id.expect("Account id of provided privkey not found!");
    log::info!("from id: {}", from_id);

    let polyjuice_validator_script_hash = {
        let mut backends = config.backends.iter();
        let polyjuice_backend = backends
            .find(|backend| backend.backend_type == BackendType::Polyjuice)
            .ok_or_else(|| anyhow!("polyjuice backend not found in config"))?;
        &polyjuice_backend.validator_script_type_hash
    };

    let mut l2_args_vec = rollup_type_hash.as_bytes().to_vec();
    l2_args_vec.append(&mut sudt_id.to_le_bytes().to_vec());
    l2_args_vec.append(&mut eth_registry_id.to_le_bytes().to_vec());
    let l2_script_args = GwPack::pack(&GwBytes::from(l2_args_vec));
    let l2_script = Script::new_builder()
        .code_hash(polyjuice_validator_script_hash.pack())
        .hash_type(ScriptHashType::Type.into())
        .args(l2_script_args)
        .build();
    let l2_script_hash = l2_script.hash();
    log::info!("l2 script hash: 0x{}", hex::encode(l2_script_hash));

    let account_id = godwoken_rpc_client
        .get_account_id_by_script_hash(l2_script_hash.into())
        .await?;
    if let Some(id) = account_id {
        log::info!("Creator account id already exists: {}", id);
        return Ok(());
    }

    let create_account = CreateAccount::new_builder()
        .script(l2_script)
        .fee(fee.pack())
        .build();

    let l2tx_args = MetaContractArgs::new_builder().set(create_account).build();
    let nonce = godwoken_rpc_client.get_nonce(from_id).await?;
    let account_raw_l2_transaction = RawL2Transaction::new_builder()
        .from_id(from_id.pack())
        .to_id(RESERVED_ACCOUNT_ID.pack())
        .nonce(nonce.pack())
        .args(l2tx_args.as_bytes().pack())
        .build();

    let sender_script_hash = godwoken_rpc_client.get_script_hash(from_id).await?;
    let receiver_script_hash = godwoken_rpc_client.get_script_hash(RESERVED_ACCOUNT_ID).await?;

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
    let tx_hash = godwoken_rpc_client.submit_l2transaction(json_bytes).await?;
    log::info!("tx hash: 0x{}", hex::encode(tx_hash.as_bytes()));

    wait_for_l2_tx(&mut godwoken_rpc_client, &tx_hash, 180, false)?;

    let account_id = godwoken_rpc_client
        .get_account_id_by_script_hash(l2_script_hash.into())
        .await?
        .expect("Creator account id not exist!");
    log::info!("Creator account id: {}", account_id);

    Ok(())
}
