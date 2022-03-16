use crate::account::{eth_sign, privkey_to_l2_script_hash, read_privkey};
use crate::godwoken_rpc::GodwokenRpcClient;
use crate::hasher::{CkbHasher, EthHasher};
use crate::types::ScriptsDeploymentResult;
use crate::utils::transaction::read_config;
use anyhow::{anyhow, Result};
use ckb_fixed_hash::H256;
use ckb_jsonrpc_types::JsonBytes;
use ckb_sdk::{Address, HumanCapacity};
use ckb_types::{prelude::Builder as CKBBuilder, prelude::Entity as CKBEntity};
use gw_common::registry_address::RegistryAddress;
use gw_types::core::ScriptHashType;
use gw_types::packed::{CellOutput, WithdrawalLockArgs, WithdrawalRequestExtra};
use gw_types::{
    packed::{Byte32, RawWithdrawalRequest, WithdrawalRequest},
    prelude::Pack as GwPack,
};
use std::str::FromStr;
use std::time::{Duration, Instant};
use std::u128;
use std::{fs, path::Path};

#[allow(clippy::too_many_arguments)]
pub async fn withdraw(
    godwoken_rpc_url: &str,
    privkey_path: &Path,
    capacity: &str,
    amount: &str,
    fee: &str,
    chain_id: &str,
    sudt_script_hash: &str,
    owner_ckb_address: &str,
    config_path: &Path,
    scripts_deployment_path: &Path,
) -> Result<()> {
    let sudt_script_hash = H256::from_str(sudt_script_hash.trim().trim_start_matches("0x"))?;
    let capacity = parse_capacity(capacity)?;
    let amount: u128 = amount.parse().expect("sUDT amount format error");
    let chain_id: u64 = chain_id.parse().expect("chain_id format error");
    let fee = parse_capacity(fee)?;

    let scripts_deployment_content = fs::read_to_string(scripts_deployment_path)?;
    let scripts_deployment: ScriptsDeploymentResult =
        serde_json::from_str(&scripts_deployment_content)?;

    let mut godwoken_rpc_client = GodwokenRpcClient::new(godwoken_rpc_url);

    let config = read_config(&config_path)?;
    let rollup_type_hash = &config.genesis.rollup_type_hash;

    let is_sudt = sudt_script_hash != H256([0u8; 32]);
    let minimal_capacity = minimal_withdrawal_capacity(is_sudt)?;
    if capacity < minimal_capacity {
        let msg = anyhow!(
            "Withdrawal required {} CKB at least, provided {}.",
            HumanCapacity::from(minimal_capacity).to_string(),
            HumanCapacity::from(capacity).to_string()
        );
        return Err(msg);
    }

    // owner_ckb_address -> owner_lock_hash
    let owner_lock_script = {
        let address = Address::from_str(owner_ckb_address).map_err(|err| anyhow!(err))?;
        let payload = address.payload();
        ckb_types::packed::Script::from(payload)
    };
    let owner_lock_hash: H256 = CkbHasher::new()
        .update(owner_lock_script.as_slice())
        .finalize();

    let privkey = read_privkey(privkey_path)?;

    let from_script_hash =
        privkey_to_l2_script_hash(&privkey, rollup_type_hash, &scripts_deployment)?;

    // get from_id
    let from_id = godwoken_rpc_client
        .get_account_id_by_script_hash(from_script_hash.clone())
        .await?;
    let from_id = from_id.expect("from id not found!");
    let nonce =
        tokio::runtime::Handle::current().block_on(godwoken_rpc_client.get_nonce(from_id))?;

    // get account_script_hash
    let account_script_hash =
        tokio::runtime::Handle::current().block_on(godwoken_rpc_client.get_script_hash(from_id))?;

    let raw_request = create_raw_withdrawal_request(
        nonce,
        capacity,
        amount,
        fee,
        chain_id,
        &sudt_script_hash,
        &account_script_hash,
        &owner_lock_hash,
    )?;

    let message = generate_withdrawal_message_to_sign(&raw_request, rollup_type_hash);
    let signature = eth_sign(&message, privkey)?;

    let withdrawal_request = WithdrawalRequest::new_builder()
        .raw(raw_request)
        .signature(signature.pack())
        .build();
    let owner_lock = gw_types::packed::Script::new_unchecked(owner_lock_script.as_bytes());
    let withdrawal_request_extra = WithdrawalRequestExtra::new_builder()
        .request(withdrawal_request)
        .owner_lock(owner_lock)
        .build();

    log::info!("withdrawal_request_extra: {}", withdrawal_request_extra);

    let from_addr = godwoken_rpc_client
        .get_registry_address_by_script_hash(&from_script_hash)
        .await?;
    let init_balance = godwoken_rpc_client.get_balance(&from_addr, 1).await?;

    let bytes = JsonBytes::from_bytes(withdrawal_request_extra.as_bytes());
    let withdrawal_hash = tokio::runtime::Handle::current()
        .block_on(godwoken_rpc_client.submit_withdrawal_request(bytes))?;
    log::info!("withdrawal_hash: {}", withdrawal_hash.pack());

    wait_for_balance_change(&mut godwoken_rpc_client, &from_addr, init_balance, 180u64).await?;

    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn create_raw_withdrawal_request(
    nonce: u32,
    capacity: u64,
    amount: u128,
    fee: u64,
    chain_id: u64,
    sudt_script_hash: &H256,
    account_script_hash: &H256,
    owner_lock_hash: &H256,
) -> Result<RawWithdrawalRequest> {
    let raw = RawWithdrawalRequest::new_builder()
        .nonce(GwPack::pack(&nonce))
        .capacity(GwPack::pack(&capacity))
        .amount(GwPack::pack(&amount))
        .sudt_script_hash(h256_to_byte32(sudt_script_hash)?)
        .account_script_hash(h256_to_byte32(account_script_hash)?)
        .owner_lock_hash(h256_to_byte32(owner_lock_hash)?)
        .fee(fee.pack())
        .chain_id(chain_id.pack())
        .build();

    Ok(raw)
}

fn h256_to_byte32(hash: &H256) -> Result<Byte32> {
    let value = Byte32::from_slice(hash.as_bytes())?;
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

async fn wait_for_balance_change(
    godwoken_rpc_client: &mut GodwokenRpcClient,
    addr: &RegistryAddress,
    init_balance: u128,
    timeout_secs: u64,
) -> Result<()> {
    let retry_timeout = Duration::from_secs(timeout_secs);
    let start_time = Instant::now();
    while start_time.elapsed() < retry_timeout {
        std::thread::sleep(Duration::from_secs(2));

        let balance = godwoken_rpc_client.get_balance(addr, 1).await?;
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
    Err(anyhow!("Timeout: {:?}", retry_timeout))
}

fn parse_capacity(capacity: &str) -> Result<u64> {
    let human_capacity = HumanCapacity::from_str(capacity).map_err(|err| anyhow!("{}", err))?;
    Ok(human_capacity.into())
}

fn minimal_withdrawal_capacity(is_sudt: bool) -> Result<u64> {
    // fixed size, the specific value is not important.
    let dummy_hash = gw_types::core::H256::zero();
    let dummy_block_number = 0u64;
    let dummy_rollup_type_hash = dummy_hash;

    let dummy_withdrawal_lock_args = WithdrawalLockArgs::new_builder()
        .account_script_hash(dummy_hash.pack())
        .withdrawal_block_hash(dummy_hash.pack())
        .withdrawal_block_number(dummy_block_number.pack())
        .owner_lock_hash(dummy_hash.pack())
        .build();

    let args: gw_types::bytes::Bytes = dummy_rollup_type_hash
        .as_slice()
        .iter()
        .chain(dummy_withdrawal_lock_args.as_slice().iter())
        .cloned()
        .collect();

    let lock_script = gw_types::packed::Script::new_builder()
        .code_hash(dummy_hash.pack())
        .hash_type(ScriptHashType::Type.into())
        .args(args.pack())
        .build();

    let type_script = if is_sudt {
        let type_ = gw_types::packed::Script::new_builder()
            .code_hash(dummy_hash.pack())
            .hash_type(ScriptHashType::Type.into())
            .args(dummy_hash.as_slice().pack())
            .build();
        Some(type_)
    } else {
        None
    };

    let output = CellOutput::new_builder()
        .capacity(0.pack())
        .lock(lock_script)
        .type_(type_script.pack())
        .build();

    let data_capacity = if is_sudt { 16 } else { 0 };

    let capacity = output.occupied_capacity(data_capacity)?;
    Ok(capacity)
}
