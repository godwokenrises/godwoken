use crate::account::{eth_sign, privkey_to_l2_script_hash, read_privkey};
use crate::godwoken_rpc::GodwokenRpcClient;
use crate::types::ScriptsDeploymentResult;
use crate::utils::message::generate_transaction_message_to_sign;
use crate::utils::transaction::{read_config, wait_for_l2_tx};
use anyhow::Result;
use ckb_jsonrpc_types::JsonBytes;
use ckb_types::bytes::Bytes;
use ckb_types::{prelude::Builder as CKBBuilder, prelude::Entity as CKBEntity};
use gw_common::builtins::ETH_REGISTRY_ACCOUNT_ID;
use gw_common::registry_address::RegistryAddress;
use gw_types::packed::{Fee, L2Transaction, RawL2Transaction, SUDTArgs, SUDTTransfer};
use gw_types::prelude::Pack as GwPack;
use gw_types::U256;
use std::path::Path;

#[allow(clippy::too_many_arguments)]
pub async fn transfer(
    godwoken_rpc_url: &str,
    privkey_path: &Path,
    to: &str,
    sudt_id: u32,
    amount: &str,
    fee: &str,
    registry_id: u32,
    config_path: &Path,
    scripts_deployment_path: &Path,
) -> Result<()> {
    let amount: U256 = amount.parse().expect("sUDT amount format error");
    let fee: u128 = fee.parse().expect("fee format error");

    let scripts_deployment_content = std::fs::read_to_string(scripts_deployment_path)?;
    let scripts_deployment: ScriptsDeploymentResult =
        serde_json::from_str(&scripts_deployment_content)?;

    let mut godwoken_rpc_client = GodwokenRpcClient::new(godwoken_rpc_url);

    let config = read_config(config_path)?;
    let rollup_type_hash = &config.genesis.rollup_type_hash;

    let privkey = read_privkey(privkey_path)?;

    // get from_id
    let from_script_hash =
        privkey_to_l2_script_hash(&privkey, rollup_type_hash, &scripts_deployment)?;
    let from_id = godwoken_rpc_client
        .get_account_id_by_script_hash(from_script_hash)
        .await?;
    let from_id = from_id.expect("from id not found!");

    let nonce = godwoken_rpc_client.get_nonce(from_id).await?;

    let to_addr = hex::decode(to.trim_start_matches("0x"))?;
    assert_eq!(to_addr.len(), 20);
    let sudt_transfer = SUDTTransfer::new_builder()
        .to_address(GwPack::pack(&Bytes::from(
            RegistryAddress::new(ETH_REGISTRY_ACCOUNT_ID, to_addr).to_bytes(),
        )))
        .amount(GwPack::pack(&amount))
        .fee(
            Fee::new_builder()
                .registry_id(GwPack::pack(&registry_id))
                .amount(GwPack::pack(&fee))
                .build(),
        )
        .build();

    let sudt_args = SUDTArgs::new_builder().set(sudt_transfer).build();

    let raw_l2transaction = RawL2Transaction::new_builder()
        .from_id(GwPack::pack(&from_id))
        .to_id(GwPack::pack(&sudt_id))
        .nonce(GwPack::pack(&nonce))
        .args(GwPack::pack(&sudt_args.as_bytes()))
        .build();

    let sender_script_hash = godwoken_rpc_client.get_script_hash(from_id).await?;
    let receiver_script_hash = godwoken_rpc_client.get_script_hash(sudt_id).await?;

    let message = generate_transaction_message_to_sign(
        &raw_l2transaction,
        rollup_type_hash,
        &sender_script_hash,
        &receiver_script_hash,
    );
    let signature = eth_sign(&message, privkey)?;

    let l2_transaction = L2Transaction::new_builder()
        .raw(raw_l2transaction)
        .signature(signature.pack())
        .build();

    log::info!("l2 transaction: {}", l2_transaction);

    let bytes = JsonBytes::from_bytes(l2_transaction.as_bytes());
    let tx_hash = tokio::runtime::Handle::current()
        .block_on(godwoken_rpc_client.submit_l2transaction(bytes))?;

    log::info!("tx_hash: 0x{}", faster_hex::hex_string(tx_hash.as_bytes())?);

    wait_for_l2_tx(&mut godwoken_rpc_client, &tx_hash, 300, false).await?;

    log::info!("transfer success!");

    Ok(())
}
