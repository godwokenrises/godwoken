use crate::account::{privkey_to_eth_address, read_privkey};
use crate::godwoken_rpc::GodwokenRpcClient;
use crate::hasher::CkbHasher;
use crate::types::ScriptsDeploymentResult;
use crate::utils::sdk::{Address, AddressPayload, HumanCapacity, SECP256K1};
use crate::utils::transaction::{get_network_type, read_config, run_cmd};
use anyhow::{anyhow, Result};
use ckb_fixed_hash::H256;
use ckb_types::core::Capacity;
use ckb_types::{bytes::Bytes as CKBBytes, core::ScriptHashType, packed::Script as CKBScript};
use gw_common::builtins::{CKB_SUDT_ACCOUNT_ID, ETH_REGISTRY_ACCOUNT_ID};
use gw_rpc_client::ckb_client::CkbClient;
use gw_types::core::Timepoint;
use gw_types::packed::{CellOutput, CustodianLockArgs};
use gw_types::U256;
use gw_types::{
    bytes::Bytes as GwBytes,
    packed::{Byte32, DepositLockArgs, Script},
    prelude::*,
};
use std::path::Path;
use std::str::FromStr;
use std::time::{Duration, Instant};

#[allow(clippy::too_many_arguments)]
pub async fn deposit_ckb(
    privkey_path: &Path,
    scripts_deployment_path: &Path,
    config_path: &Path,
    capacity: &str,
    fee: &str,
    ckb_rpc_url: &str,
    eth_address: Option<&str>,
    godwoken_rpc_url: &str,
) -> Result<()> {
    let scripts_deployment_content = std::fs::read_to_string(scripts_deployment_path)?;
    let scripts_deployment: ScriptsDeploymentResult =
        serde_json::from_str(&scripts_deployment_content)?;

    let config = read_config(&config_path)?;

    let privkey = read_privkey(privkey_path)?;

    // Using private key to calculate eth address when eth_address not provided.
    let eth_address_bytes = match eth_address {
        Some(addr) => {
            let addr_vec = hex::decode(&addr.trim_start_matches("0x").as_bytes())?;
            CKBBytes::from(addr_vec)
        }
        None => privkey_to_eth_address(&privkey)?,
    };
    log::info!("eth address: 0x{:#x}", eth_address_bytes);

    let rollup_type_hash = &config.consensus.get_config().genesis.rollup_type_hash;

    let owner_lock_hash = Byte32::from_slice(privkey_to_lock_hash(&privkey)?.as_bytes())?;

    // build layer2 lock
    let l2_code_hash = &scripts_deployment.eth_account_lock.script_type_hash;

    let mut l2_args_vec = rollup_type_hash.as_bytes().to_vec();
    l2_args_vec.append(&mut eth_address_bytes.to_vec());
    let l2_lock_args = Pack::pack(&GwBytes::from(l2_args_vec));

    let l2_lock = Script::new_builder()
        .code_hash(Byte32::from_slice(l2_code_hash.as_bytes())?)
        .hash_type(ScriptHashType::Type.into())
        .args(l2_lock_args)
        .build();

    let l2_lock_hash = CkbHasher::new().update(l2_lock.as_slice()).finalize();

    let l2_lock_hash_str = format!("0x{}", faster_hex::hex_string(l2_lock_hash.as_bytes())?);
    log::info!("layer2 script hash: {}", l2_lock_hash_str);

    // cancel_timeout default to 20 minutes
    let deposit_lock_args = DepositLockArgs::new_builder()
        .owner_lock_hash(owner_lock_hash)
        .cancel_timeout(Pack::pack(&0xc0000000000004b0u64))
        .layer2_lock(l2_lock)
        .registry_id(Pack::pack(&ETH_REGISTRY_ACCOUNT_ID))
        .build();

    let minimal_capacity = minimal_deposit_capacity(&deposit_lock_args)?;
    let capacity_in_shannons = parse_capacity(capacity)?;
    if capacity_in_shannons < minimal_capacity {
        let msg = anyhow!(
            "Deposit CKB required {} CKB at least, provided {}.",
            HumanCapacity::from(minimal_capacity).to_string(),
            HumanCapacity::from(capacity_in_shannons).to_string()
        );
        return Err(msg);
    }

    let mut l1_lock_args = rollup_type_hash.as_bytes().to_vec();
    l1_lock_args.append(&mut deposit_lock_args.as_bytes().to_vec());

    let deposit_lock_code_hash = &scripts_deployment.deposit_lock.script_type_hash;

    let rpc_client = CkbClient::with_url(ckb_rpc_url)?;
    let network_type = get_network_type(&rpc_client).await?;
    let address_payload = AddressPayload::new_full(
        ScriptHashType::Type,
        Pack::pack(deposit_lock_code_hash),
        GwBytes::from(l1_lock_args),
    );
    let address: Address = Address::new(network_type, address_payload, true);

    let mut godwoken_rpc_client = GodwokenRpcClient::new(godwoken_rpc_url);

    log::info!("script hash: 0x{}", hex::encode(l2_lock_hash.as_bytes()));

    let init_balance = get_balance_by_script_hash(&mut godwoken_rpc_client, &l2_lock_hash).await?;
    log::info!("balance before deposit: {}", init_balance);

    loop {
        let result = run_cmd(vec![
            "--url",
            ckb_rpc_url,
            "wallet",
            "transfer",
            "--privkey-path",
            privkey_path.to_str().expect("non-utf8 file path"),
            "--to-address",
            address.to_string().as_str(),
            "--capacity",
            capacity,
            "--tx-fee",
            fee,
            "--skip-check-to-address",
        ]);
        let output = match result {
            Ok(output) => output,
            Err(e) => {
                // Sending transaction may fail because there is another
                // **proposed** but not committed transaction using the same
                // input.
                log::warn!("Running ckb-cli failed: {:?}. Retrying.", e);
                tokio::time::sleep(Duration::from_secs(3)).await;
                continue;
            }
        };

        let tx_hash = H256::from_str(output.trim().trim_start_matches("0x"))?;
        log::info!("tx_hash: {:#x}", tx_hash);

        if let Err(e) = rpc_client
            .wait_tx_committed_with_timeout_and_logging(tx_hash.0, 600)
            .await
        {
            if e.to_string().contains("rejected") {
                // Transaction can be rejected due to double spending. Retry.
                log::warn!("Transaction is rejected. Retrying.");
            } else {
                return Err(e);
            }
        } else {
            break;
        }
    }

    wait_for_balance_change(
        &mut godwoken_rpc_client,
        &l2_lock_hash,
        init_balance,
        180u64,
    )
    .await?;

    Ok(())
}

fn privkey_to_lock_hash(privkey: &H256) -> Result<H256> {
    let privkey = secp256k1::SecretKey::from_slice(privkey.as_bytes())?;
    let pubkey = secp256k1::PublicKey::from_secret_key(&SECP256K1, &privkey);
    let address_payload = AddressPayload::from_pubkey(&pubkey);

    let lock_hash: H256 = CKBScript::from(&address_payload)
        .calc_script_hash()
        .unpack();
    Ok(lock_hash)
}

async fn wait_for_balance_change(
    godwoken_rpc_client: &mut GodwokenRpcClient,
    from_script_hash: &H256,
    init_balance: U256,
    timeout_secs: u64,
) -> Result<()> {
    let retry_timeout = Duration::from_secs(timeout_secs);
    let start_time = Instant::now();
    while start_time.elapsed() < retry_timeout {
        std::thread::sleep(Duration::from_secs(2));

        let balance = get_balance_by_script_hash(godwoken_rpc_client, from_script_hash).await?;
        log::info!(
            "current balance: {}, waiting for {} secs.",
            balance,
            start_time.elapsed().as_secs()
        );

        if balance != init_balance {
            log::info!("deposit success!");
            let account_id = godwoken_rpc_client
                .get_account_id_by_script_hash(from_script_hash.clone())
                .await?
                .unwrap();
            log::info!("Your account id: {}", account_id);
            return Ok(());
        }
    }
    Err(anyhow!("Timeout: {:?}", retry_timeout))
}

async fn get_balance_by_script_hash(
    godwoken_rpc_client: &mut GodwokenRpcClient,
    script_hash: &H256,
) -> Result<U256> {
    let addr = godwoken_rpc_client
        .get_registry_address_by_script_hash(script_hash)
        .await;
    let balance = match addr {
        Ok(Some(reg_addr)) => {
            godwoken_rpc_client
                .get_balance(&reg_addr, CKB_SUDT_ACCOUNT_ID)
                .await?
        }
        Ok(None) => U256::zero(),
        Err(e) => {
            log::warn!("failed to get_registry_address_by_script_hash, {}", e);
            U256::zero()
        }
    };
    Ok(balance)
}

// only for CKB
fn minimal_deposit_capacity(deposit_lock_args: &DepositLockArgs) -> Result<u64> {
    use gw_types::h256::H256Ext;

    // fixed size, the specific value is not important.
    let dummy_hash = gw_types::core::H256::zero();
    let dummy_timepoint = Timepoint::from_block_number(0);
    let dummy_rollup_type_hash = dummy_hash;

    let custodian_lock_args = CustodianLockArgs::new_builder()
        .deposit_block_hash(Pack::pack(&dummy_hash))
        .deposit_finalized_timepoint(Pack::pack(&dummy_timepoint.full_value()))
        .deposit_lock_args(deposit_lock_args.clone())
        .build();

    let args: gw_types::bytes::Bytes = dummy_rollup_type_hash
        .as_slice()
        .iter()
        .chain(custodian_lock_args.as_slice().iter())
        .cloned()
        .collect();

    let lock_script = Script::new_builder()
        .code_hash(Pack::pack(&dummy_hash))
        .hash_type(ScriptHashType::Type.into())
        .args(Pack::pack(&args))
        .build();

    // no type / data when deposit CKB
    let output = CellOutput::new_builder()
        .capacity(Pack::pack(&0))
        .lock(lock_script)
        .build();

    let capacity = output.occupied_capacity(Capacity::zero())?;
    Ok(capacity.as_u64())
}

fn parse_capacity(capacity: &str) -> Result<u64> {
    let human_capacity = HumanCapacity::from_str(capacity).map_err(|err| anyhow!(err))?;
    Ok(human_capacity.into())
}
