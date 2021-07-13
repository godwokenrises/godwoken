use crate::account::{
    eth_sign, privkey_to_short_address, read_privkey, short_address_to_account_id,
};
use crate::deploy_scripts::ScriptsDeploymentResult;
use crate::godwoken_rpc::GodwokenRpcClient;
use crate::hasher::{CkbHasher, EthHasher};
use crate::utils::read_config;
use ckb_fixed_hash::H256;
use ckb_jsonrpc_types::JsonBytes;
use ckb_sdk::{Address, HumanCapacity};
use ckb_types::{prelude::Builder as CKBBuilder, prelude::Entity as CKBEntity};
use gw_types::{
    bytes::Bytes as GwBytes,
    packed::{Byte32, RawWithdrawalRequest, WithdrawalRequest},
    prelude::Pack as GwPack,
};
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
        fs::read_to_string(deployment_results_path).map_err(|err| err.to_string())?;
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

        CkbHasher::new()
            .update(owner_lock_script.as_slice())
            .finalize()
    };

    let privkey = read_privkey(privkey_path)?;

    let from_address = privkey_to_short_address(&privkey, &rollup_type_hash, &deployment_result)?;

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
    let signature = eth_sign(&message, privkey)?;

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

    let digest = CkbHasher::new()
        .update(rollup_type_hash_data)
        .update(raw_data)
        .finalize();

    let message = EthHasher::new()
        .update("\x19Ethereum Signed Message:\n32")
        .update(digest.as_bytes())
        .finalize();

    message
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
