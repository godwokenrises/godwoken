use crate::deploy_scripts::ScriptsDeploymentResult;
use crate::deposit_ckb::{privkey_to_eth_address, read_config};
use crate::godwoken_rpc::GodwokenRpcClient;
use ckb_crypto::secp::Privkey;
use ckb_fixed_hash::H256;
use ckb_jsonrpc_types::JsonBytes;
use ckb_sdk::{Address, HumanCapacity};
use ckb_types::{
    core::ScriptHashType, prelude::Builder as CKBBuilder, prelude::Entity as CKBEntity,
};
use gw_common::blake2b::new_blake2b;
use gw_types::{
    bytes::Bytes as GwBytes,
    packed::{Byte32, RawWithdrawalRequest, Script, WithdrawalRequest},
    prelude::Pack as GwPack,
};
use sha3::{Digest, Keccak256};
use std::str::FromStr;
use std::time::{Duration, Instant};
use std::u128;
use std::{fs, path::Path};

#[allow(clippy::too_many_arguments)]
pub fn withdraw(
    godwoken_rpc_url: &str,
    privkey_path: &Path,
    capacity: &str,
    amount: &str,
    sudt_script_hash: &str,
    owner_ckb_address: &str,
    config_path: &Path,
    deployment_results_path: &Path,
) -> Result<(), String> {
    let sudt_script_hash =
        H256::from_str(&sudt_script_hash.trim()[2..]).map_err(|err| err.to_string())?;
    let capacity = parse_capacity(capacity)?;
    let amount: u128 = amount.parse().expect("sUDT amount format error");

    let deployment_result_string =
        std::fs::read_to_string(deployment_results_path).map_err(|err| err.to_string())?;
    let deployment_result: ScriptsDeploymentResult =
        serde_json::from_str(&deployment_result_string).map_err(|err| err.to_string())?;

    let mut godwoken_rpc_client = GodwokenRpcClient::new(godwoken_rpc_url);

    let sell_capacity = 100u64 * 10u64.pow(8);

    let config = read_config(&config_path)?;
    let rollup_type_hash = &config.genesis.rollup_type_hash;

    let payment_lock_hash = H256::from([0u8; 32]);

    // owner_ckb_address -> owner_lock_hash
    let owner_lock_hash: H256 = {
        let address = Address::from_str(owner_ckb_address)?;
        let payload = address.payload();
        let owner_lock_script = ckb_types::packed::Script::from(payload);

        let mut hasher = new_blake2b();
        hasher.update(owner_lock_script.as_slice());
        let mut hash = [0u8; 32];
        hasher.finalize(&mut hash);
        hash.into()
    };

    let privkey_string = fs::read_to_string(privkey_path)
        .map_err(|err| err.to_string())?
        .split_whitespace()
        .next()
        .map(ToOwned::to_owned)
        .ok_or_else(|| "Privkey file is empty".to_string())?;
    let privkey = H256::from_str(&privkey_string.trim()[2..]).map_err(|err| err.to_string())?;

    let from_address =
        privkey_to_short_address(&privkey_string, &rollup_type_hash, &deployment_result)?;

    // get from_id
    let from_id = short_address_to_account_id(&mut godwoken_rpc_client, &from_address)?;
    let from_id = from_id.expect("from id not found!");
    let nonce = godwoken_rpc_client.get_nonce(from_id)?;

    // get account_script_hash
    let account_script_hash = godwoken_rpc_client.get_script_hash(from_id)?;

    let raw_request = create_raw_withdrawal_request(
        &nonce,
        &capacity,
        &amount,
        &sudt_script_hash,
        &account_script_hash,
        &sell_capacity,
        &0u128,
        &owner_lock_hash,
        &payment_lock_hash,
    )?;

    let message = generate_withdrawal_message_to_sign(&raw_request, rollup_type_hash);
    let signature = get_signature(&message, privkey)?;

    let withdrawal_request = WithdrawalRequest::new_builder()
        .raw(raw_request)
        .signature(signature.pack())
        .build();

    log::info!("withdrawal_request: {}", withdrawal_request);

    let init_balance =
        godwoken_rpc_client.get_balance(JsonBytes::from_bytes(from_address.clone()), 1)?;

    let bytes = JsonBytes::from_bytes(withdrawal_request.as_bytes());
    godwoken_rpc_client.submit_withdrawal_request(bytes)?;

    wait_for_balance_change(&mut godwoken_rpc_client, from_address, init_balance, 180u64)?;

    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn create_raw_withdrawal_request(
    nonce: &u32,
    capacity: &u64,
    amount: &u128,
    sudt_script_hash: &H256,
    account_script_hash: &H256,
    sell_capacity: &u64,
    sell_amount: &u128,
    owner_lock_hash: &H256,
    payment_lock_hash: &H256,
) -> Result<RawWithdrawalRequest, String> {
    let raw = RawWithdrawalRequest::new_builder()
        .nonce(GwPack::pack(nonce))
        .capacity(GwPack::pack(capacity))
        .amount(GwPack::pack(amount))
        .sudt_script_hash(h256_to_byte32(sudt_script_hash)?)
        .account_script_hash(h256_to_byte32(account_script_hash)?)
        .sell_capacity(GwPack::pack(sell_capacity))
        .sell_amount(GwPack::pack(sell_amount))
        .owner_lock_hash(h256_to_byte32(owner_lock_hash)?)
        .payment_lock_hash(h256_to_byte32(payment_lock_hash)?)
        .build();

    Ok(raw)
}

fn h256_to_byte32(hash: &H256) -> Result<Byte32, String> {
    let value = Byte32::from_slice(hash.as_bytes()).map_err(|err| err.to_string())?;
    Ok(value)
}

fn generate_withdrawal_message_to_sign(
    raw_request: &RawWithdrawalRequest,
    rollup_type_hash: &H256,
) -> H256 {
    let raw_data = raw_request.as_slice();
    let rollup_type_hash_data = rollup_type_hash.as_bytes();

    let digest: H256 = {
        let mut hasher = new_blake2b();
        hasher.update(rollup_type_hash_data);
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

fn sign_message(msg: &H256, privkey_data: H256) -> Result<[u8; 65], String> {
    let privkey = Privkey::from(privkey_data);
    let signature = privkey
        .sign_recoverable(msg)
        .map_err(|err| err.to_string())?;
    let mut inner = [0u8; 65];
    inner.copy_from_slice(&signature.serialize());
    Ok(inner)
}

pub fn get_signature(msg: &H256, privkey: H256) -> Result<[u8; 65], String> {
    let mut signature = sign_message(msg, privkey)?;
    let v = &mut signature[64];
    if *v >= 27 {
        *v -= 27;
    }
    Ok(signature)
}

pub fn privkey_to_short_address(
    privkey: &str,
    rollup_type_hash: &H256,
    deployment_result: &ScriptsDeploymentResult,
) -> Result<GwBytes, String> {
    let eth_address = privkey_to_eth_address(privkey)?;

    let code_hash = Byte32::from_slice(
        deployment_result
            .eth_account_lock
            .script_type_hash
            .as_bytes(),
    )
    .map_err(|err| err.to_string())?;

    let mut args_vec = rollup_type_hash.as_bytes().to_vec();
    args_vec.append(&mut eth_address.to_vec());
    let args = GwPack::pack(&GwBytes::from(args_vec));

    let script = Script::new_builder()
        .code_hash(code_hash)
        .hash_type(ScriptHashType::Type.into())
        .args(args)
        .build();

    let script_hash: H256 = {
        let mut hasher = new_blake2b();
        hasher.update(script.as_slice());
        let mut hash = [0u8; 32];
        hasher.finalize(&mut hash);
        hash.into()
    };

    let short_address = &script_hash.as_bytes()[..20];

    let addr = GwBytes::from(short_address.to_vec());

    Ok(addr)
}

pub fn short_address_to_account_id(
    godwoken_rpc_client: &mut GodwokenRpcClient,
    short_address: &GwBytes,
) -> Result<Option<u32>, String> {
    let bytes = JsonBytes::from_bytes(short_address.clone());
    let script_hash = godwoken_rpc_client.get_script_hash_by_short_address(bytes)?;
    let account_id = godwoken_rpc_client.get_account_id_by_script_hash(script_hash)?;

    Ok(account_id)
}

fn wait_for_balance_change(
    godwoken_rpc_client: &mut GodwokenRpcClient,
    from_address: GwBytes,
    init_balance: u128,
    timeout_secs: u64,
) -> Result<(), String> {
    let retry_timeout = Duration::from_secs(timeout_secs);
    let start_time = Instant::now();
    while start_time.elapsed() < retry_timeout {
        std::thread::sleep(Duration::from_secs(2));

        let balance =
            godwoken_rpc_client.get_balance(JsonBytes::from_bytes(from_address.clone()), 1)?;
        log::info!(
            "current balance: {}, waiting for {} secs.",
            balance,
            start_time.elapsed().as_secs()
        );

        if balance != init_balance {
            log::info!("withdraw success!");
            return Ok(());
        }
    }
    Err(format!("Timeout: {:?}", retry_timeout))
}

fn parse_capacity(capacity: &str) -> Result<u64, String> {
    let human_capacity = HumanCapacity::from_str(capacity)?;
    Ok(human_capacity.into())
}
