use anyhow::{anyhow, Result};
use ckb_jsonrpc_types::JsonBytes;
use ckb_types::prelude::{Builder, Entity};
use gw_common::builtins::{ETH_REGISTRY_ACCOUNT_ID, RESERVED_ACCOUNT_ID};
use gw_config::BackendType;
use gw_generator::account_lock_manage::eip712::{self, traits::EIP712Encode};
use gw_types::{
    core::ScriptHashType,
    packed::{CreateAccount, Fee, L2Transaction, MetaContractArgs, RawL2Transaction, Script},
};
use std::path::Path;

use crate::{
    account::{eth_sign, privkey_to_eth_address, privkey_to_l2_script_hash, read_privkey},
    godwoken_rpc::GodwokenRpcClient,
    types::ScriptsDeploymentResult,
    utils::transaction::{read_config, wait_for_l2_tx},
};
use gw_types::{bytes::Bytes as GwBytes, prelude::Pack as GwPack};

pub async fn create_creator_account(
    godwoken_rpc_url: &str,
    privkey_path: &Path,
    sudt_id: u32,
    fee_amount: &str,
    config_path: &Path,
    scripts_deployment_path: &Path,
) -> Result<()> {
    let fee: u128 = fee_amount.parse().expect("fee format error");

    let scripts_deployment_content = std::fs::read_to_string(scripts_deployment_path)?;
    let scripts_deployment: ScriptsDeploymentResult =
        serde_json::from_str(&scripts_deployment_content)?;

    let mut godwoken_rpc_client = GodwokenRpcClient::new(godwoken_rpc_url);

    let config = read_config(config_path)?;
    let rollup_type_hash = &config.genesis.rollup_type_hash;
    let privkey = read_privkey(privkey_path)?;

    let from_script_hash =
        privkey_to_l2_script_hash(&privkey, rollup_type_hash, &scripts_deployment)?;
    let from_id = godwoken_rpc_client
        .get_account_id_by_script_hash(from_script_hash)
        .await?;
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
        .fee(
            Fee::new_builder()
                .amount(fee.pack())
                .registry_id(ETH_REGISTRY_ACCOUNT_ID.pack())
                .build(),
        )
        .build();

    let l2tx_args = MetaContractArgs::new_builder().set(create_account).build();
    let nonce = godwoken_rpc_client.get_nonce(from_id).await?;
    let chain_id: u64 = config.genesis.rollup_config.chain_id.into();
    let raw_l2_transaction = RawL2Transaction::new_builder()
        .chain_id(chain_id.pack())
        .from_id(from_id.pack())
        .to_id(RESERVED_ACCOUNT_ID.pack())
        .nonce(nonce.pack())
        .args(l2tx_args.as_bytes().pack())
        .build();

    let from_address = privkey_to_eth_address(&privkey)
        .expect("privkey_to_eth_address")
        .to_vec();
    let sender_address =
        gw_common::registry_address::RegistryAddress::new(ETH_REGISTRY_ACCOUNT_ID, from_address);
    let receiver_script_hash: [u8; 32] = godwoken_rpc_client
        .get_script_hash(RESERVED_ACCOUNT_ID)
        .await?
        .into();
    let message = {
        let typed_tx = eip712::types::L2Transaction::from_raw(
            raw_l2_transaction.clone(),
            sender_address,
            receiver_script_hash.into(),
        )
        .unwrap();
        let domain_seperator = eip712::types::EIP712Domain {
            name: "Godwoken".to_string(),
            version: "1".to_string(),
            chain_id,
            verifying_contract: None,
            salt: None,
        };
        typed_tx.eip712_message(EIP712Encode::hash_struct(&domain_seperator))
    };
    let signature = eth_sign(&message.into(), privkey)?;
    let account_l2_transaction = L2Transaction::new_builder()
        .raw(raw_l2_transaction)
        .signature(signature.pack())
        .build();

    let json_bytes = JsonBytes::from_bytes(account_l2_transaction.as_bytes());
    let tx_hash = godwoken_rpc_client.submit_l2transaction(json_bytes).await?;
    log::info!("tx hash: 0x{}", hex::encode(tx_hash.as_bytes()));

    wait_for_l2_tx(&mut godwoken_rpc_client, &tx_hash, 180, false).await?;

    let account_id = godwoken_rpc_client
        .get_account_id_by_script_hash(l2_script_hash.into())
        .await?
        .expect("Creator account id not exist!");
    // Kicker relies on this output to determine that creator account creation
    // is successful.
    println!("Creator account id: {}", account_id);

    Ok(())
}
