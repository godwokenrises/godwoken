use anyhow::{anyhow, Result};
use ckb_fixed_hash::H256;
use ckb_jsonrpc_types::JsonBytes;
use ckb_types::prelude::{Builder, Entity};
use gw_config::{BackendType, Config};
use gw_types::{
    core::ScriptHashType,
    packed::{CreateAccount, Fee, L2Transaction, MetaContractArgs, RawL2Transaction, Script},
};

use crate::{
    account::{eth_sign, privkey_to_l2_script_hash},
    godwoken_rpc::GodwokenRpcClient,
    types::ScriptsDeploymentResult,
    utils::{message::generate_eip712_message_to_sign, transaction::wait_for_l2_tx},
};
use gw_types::prelude::*;

pub fn build_l1_sudt_type_script(
    l1_sudt_script_args: &H256,
    l1_sudt_script_code_hash: &H256,
) -> Script {
    Script::new_builder()
        .args(l1_sudt_script_args.as_bytes().to_vec().pack())
        .code_hash(l1_sudt_script_code_hash.pack())
        .hash_type(ScriptHashType::Type.into())
        .build()
}

fn build_l2_sudt_script(
    rollup_script_hash: &H256,
    l2_sudt_type_hash: &H256,
    l1_sudt_script_hash: &H256,
) -> Script {
    let args = {
        let mut args = Vec::with_capacity(64);
        args.extend(rollup_script_hash.as_bytes());
        args.extend(l1_sudt_script_hash.as_bytes());
        args
    };
    Script::new_builder()
        .args(args[..].pack())
        .code_hash(l2_sudt_type_hash.pack())
        .hash_type(ScriptHashType::Type.into())
        .build()
}

async fn pk_to_account_id(
    rpc_client: &mut GodwokenRpcClient,
    rollup_type_hash: &H256,
    deployment: &ScriptsDeploymentResult,
    pk: &H256,
) -> Result<u32> {
    let from_script_hash = privkey_to_l2_script_hash(pk, rollup_type_hash, deployment)
        .map_err(|err| anyhow!("{}", err))?;
    let from_id = rpc_client
        .get_account_id_by_script_hash(from_script_hash)
        .await
        .map_err(|err| anyhow!("{}", err))?;
    Ok(from_id.expect("Account id of provided privkey not found!"))
}

#[allow(clippy::too_many_arguments)]
pub async fn create_sudt_account(
    rpc_client: &mut GodwokenRpcClient,
    pk: &H256,
    sudt_type_hash: H256,
    fee: u128,
    config: &Config,
    deployment: &ScriptsDeploymentResult,
    registry_id: u32,
    quiet: bool,
) -> Result<u32> {
    let consensus = config.consensus.get_config();
    let rollup_type_hash = &consensus.genesis.rollup_type_hash;

    let from_id = pk_to_account_id(rpc_client, rollup_type_hash, deployment, pk).await?;
    let nonce = rpc_client
        .get_nonce(from_id)
        .await
        .map_err(|err| anyhow!("{}", err))?;

    // sudt contract
    let l2_script = {
        let l2_validator_script_hash = &consensus.backend_forks[0]
            .backends
            .iter()
            .find(|b| b.backend_type == BackendType::Sudt)
            .expect("sudt")
            .validator_script_type_hash;
        build_l2_sudt_script(rollup_type_hash, l2_validator_script_hash, &sudt_type_hash)
    };
    let l2_script_hash = l2_script.hash();
    if !quiet {
        log::info!("l2 script hash: 0x{}", hex::encode(l2_script_hash));
    }

    let account_id = rpc_client
        .get_account_id_by_script_hash(l2_script_hash.into())
        .await
        .map_err(|err| anyhow!("{}", err))?;
    if let Some(id) = account_id {
        if !quiet {
            log::info!("Simple UDT account id already exists: {}", id);
        }
        return Ok(id);
    }

    let create_account = CreateAccount::new_builder()
        .script(l2_script)
        .fee(
            Fee::new_builder()
                .amount(fee.pack())
                .registry_id(registry_id.pack())
                .build(),
        )
        .build();

    let args = MetaContractArgs::new_builder().set(create_account).build();
    let chain_id: u64 = consensus.genesis.rollup_config.chain_id.into();

    let account_raw_l2_transaction = RawL2Transaction::new_builder()
        .from_id(from_id.pack())
        .to_id(0u32.pack())
        .nonce(nonce.pack())
        .args(args.as_bytes().pack())
        .chain_id(chain_id.pack())
        .build();

    let sender_address = {
        let sender_script_hash = rpc_client
            .get_script_hash(from_id)
            .await
            .map_err(|err| anyhow!("{}", err))?;
        rpc_client
            .get_registry_address_by_script_hash(&sender_script_hash)
            .await?
            .expect("sender address")
    };
    let receiver_script_hash = {
        let hash = rpc_client
            .get_script_hash(0)
            .await
            .map_err(|err| anyhow!("{}", err))?;
        hash.into()
    };

    let message = generate_eip712_message_to_sign(
        account_raw_l2_transaction.clone(),
        sender_address,
        receiver_script_hash,
    );

    let signature = eth_sign(&message, pk.to_owned()).map_err(|err| anyhow!("{}", err))?;
    let account_l2_transaction = L2Transaction::new_builder()
        .raw(account_raw_l2_transaction)
        .signature(signature.pack())
        .build();

    let json_bytes = JsonBytes::from_bytes(account_l2_transaction.as_bytes());
    let tx_hash = rpc_client
        .submit_l2transaction(json_bytes)
        .await
        .map_err(|err| anyhow!("{}", err))?;
    if !quiet {
        log::info!("tx hash: 0x{}", hex::encode(tx_hash.as_bytes()));
    }

    wait_for_l2_tx(rpc_client, &tx_hash, 180, quiet)
        .await
        .map_err(|err| anyhow!("{}", err))?;

    let account_id;
    loop {
        if let Some(id) = rpc_client
            .get_account_id_by_script_hash(l2_script_hash.into())
            .await
            .map_err(|err| anyhow!("{}", err))?
        {
            account_id = id;
            break;
        }
        if !quiet {
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        }
    }

    if !quiet {
        log::info!("Simple UDT account id: {}", account_id);
    }

    Ok(account_id)
}
