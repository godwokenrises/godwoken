use crate::deploy_scripts::ScriptsDeploymentResult;
use crate::deposit_ckb::read_config;
use crate::godwoken_rpc::GodwokenRpcClient;
use crate::withdraw::{get_signature, privkey_to_short_address, short_address_to_account_id};
use ckb_fixed_hash::H256;
use ckb_jsonrpc_types::JsonBytes;
use ckb_types::{prelude::Builder as CKBBuilder, prelude::Entity as CKBEntity};
use gw_common::blake2b::new_blake2b;
use gw_types::packed::{L2Transaction, RawL2Transaction, SUDTArgs, SUDTTransfer};
use gw_types::{bytes::Bytes as GwBytes, prelude::Pack as GwPack};
use sha3::{Digest, Keccak256};
use std::str::FromStr;
use std::time::{Duration, Instant};
use std::u128;
use std::{fs, path::Path};

#[allow(clippy::too_many_arguments)]
pub fn transfer(
    godwoken_rpc_url: &str,
    privkey_path: &Path,
    to: &str,
    sudt_id: u32,
    amount: &str,
    fee: &str,
    config_path: &Path,
    deployment_results_path: &Path,
) -> Result<(), String> {
    let amount: u128 = amount.parse().expect("sUDT amount format error");
    let fee: u128 = fee.parse().expect("fee format error");

    let deployment_result_string =
        std::fs::read_to_string(deployment_results_path).map_err(|err| err.to_string())?;
    let deployment_result: ScriptsDeploymentResult =
        serde_json::from_str(&deployment_result_string).map_err(|err| err.to_string())?;

    let mut godwoken_rpc_client = GodwokenRpcClient::new(godwoken_rpc_url);

    let to_address = parse_account_short_address(&mut godwoken_rpc_client, to)?;

    let config = read_config(&config_path)?;
    let rollup_type_hash = &config.genesis.rollup_type_hash;

    let privkey_string = fs::read_to_string(privkey_path)
        .map_err(|err| err.to_string())?
        .split_whitespace()
        .next()
        .map(ToOwned::to_owned)
        .ok_or_else(|| "Privkey file is empty".to_string())?;
    let privkey = H256::from_str(&privkey_string.trim()[2..]).map_err(|err| err.to_string())?;

    // get from_id
    let from_address =
        privkey_to_short_address(&privkey_string, &rollup_type_hash, &deployment_result)?;
    let from_id = short_address_to_account_id(&mut godwoken_rpc_client, &from_address)?;
    let from_id = from_id.expect("from id not found!");

    let nonce = godwoken_rpc_client.get_nonce(from_id)?;

    let sudt_transfer = SUDTTransfer::new_builder()
        .to(GwPack::pack(&to_address))
        .amount(GwPack::pack(&amount))
        .fee(GwPack::pack(&fee))
        .build();

    let sudt_args = SUDTArgs::new_builder().set(sudt_transfer).build();

    let raw_l2transaction = RawL2Transaction::new_builder()
        .from_id(GwPack::pack(&from_id))
        .to_id(GwPack::pack(&sudt_id))
        .nonce(GwPack::pack(&nonce))
        .args(GwPack::pack(&sudt_args.as_bytes()))
        .build();

    let sender_script_hash = godwoken_rpc_client.get_script_hash(from_id)?;
    let receiver_script_hash = godwoken_rpc_client.get_script_hash(sudt_id)?;

    let message = generate_transaction_message_to_sign(
        &raw_l2transaction,
        rollup_type_hash,
        &sender_script_hash,
        &receiver_script_hash,
    );
    let signature = get_signature(&message, privkey)?;

    let l2_transaction = L2Transaction::new_builder()
        .raw(raw_l2transaction)
        .signature(signature.pack())
        .build();

    log::info!("l2 transaction: {}", l2_transaction);

    let bytes = JsonBytes::from_bytes(l2_transaction.as_bytes());
    let tx_hash = godwoken_rpc_client.submit_l2transaction(bytes)?;

    log::info!(
        "tx_hash: 0x{}",
        faster_hex::hex_string(tx_hash.as_bytes()).map_err(|err| err.to_string())?
    );

    wait_for_tx_commit(&mut godwoken_rpc_client, &tx_hash, 300)?;

    log::info!("transfer success!");

    Ok(())
}

fn generate_transaction_message_to_sign(
    raw_l2transaction: &RawL2Transaction,
    rollup_type_hash: &H256,
    sender_script_hash: &H256,
    receiver_script_hash: &H256,
) -> H256 {
    let raw_data = raw_l2transaction.as_slice();
    let rollup_type_hash_data = rollup_type_hash.as_bytes();

    let digest: H256 = {
        let mut hasher = new_blake2b();
        hasher.update(rollup_type_hash_data);
        hasher.update(sender_script_hash.as_bytes());
        hasher.update(receiver_script_hash.as_bytes());
        hasher.update(raw_data);
        let mut hash = [0u8; 32];
        hasher.finalize(&mut hash);
        hash.into()
    };

    let message = {
        let mut hasher = Keccak256::new();
        hasher.update("\x19Ethereum Signed Message:\n32");
        hasher.update(digest.as_bytes());
        let buf = hasher.finalize();
        let mut signing_message = [0u8; 32];
        signing_message.copy_from_slice(&buf[..]);
        H256::from(signing_message)
    };

    message
}

fn wait_for_tx_commit(
    godwoken_rpc_client: &mut GodwokenRpcClient,
    tx_hash: &H256,
    timeout_secs: u64,
) -> Result<(), String> {
    let retry_timeout = Duration::from_secs(timeout_secs);
    let start_time = Instant::now();
    while start_time.elapsed() < retry_timeout {
        std::thread::sleep(Duration::from_secs(2));

        let receipt = godwoken_rpc_client.get_transaction_receipt(tx_hash)?;

        match receipt {
            Some(_) => {
                log::info!("tx committed");
                return Ok(());
            }
            None => {
                log::info!("waiting for {} secs.", start_time.elapsed().as_secs());
            }
        }
    }
    Err(format!("Timeout: {:?}", retry_timeout))
}

// address: 0x... / id: 1
fn parse_account_short_address(
    godwoken: &mut GodwokenRpcClient,
    account: &str,
) -> Result<GwBytes, String> {
    // if match short address
    if account.starts_with("0x") && account.len() == 42 {
        let r = GwBytes::from(hex::decode(account[2..].as_bytes()).map_err(|err| err.to_string())?);
        return Ok(r);
    }

    // if match id
    let account_id: u32 = account.parse().expect("account id parse error!");
    let script_hash = godwoken.get_script_hash(account_id)?;
    let short_address = GwBytes::from((&script_hash.as_bytes()[..20]).to_vec());
    Ok(short_address)
}
