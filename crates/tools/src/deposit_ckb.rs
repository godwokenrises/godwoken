use crate::deploy_scripts::wait_for_tx;
use crate::deploy_scripts::{get_network_type, run_cmd, ScriptsDeploymentResult};
use crate::godwoken_rpc::GodwokenRpcClient;
use ckb_fixed_hash::H256;
use ckb_sdk::{Address, AddressPayload, HttpRpcClient, SECP256K1};
use ckb_types::{
    bytes::Bytes as CKBBytes, core::ScriptHashType, packed::Script as CKBScript,
    prelude::Builder as CKBBuilder, prelude::Entity as CKBEntity, prelude::Pack as CKBPack,
    prelude::Unpack as CKBUnpack,
};
use gw_common::blake2b::new_blake2b;
use gw_config::Config;
use gw_types::{
    bytes::Bytes as GwBytes,
    packed::{Byte32, DepositLockArgs, Script},
    prelude::Pack as GwPack,
};
use sha3::{Digest, Keccak256};
use std::str::FromStr;
use std::time::{Duration, Instant};
use std::u128;
use std::{fs, path::Path};

#[allow(clippy::too_many_arguments)]
pub fn deposit_ckb(
    privkey_path: &Path,
    deployment_results_path: &Path,
    config_path: &Path,
    capacity: &str,
    fee: &str,
    ckb_rpc_url: &str,
    eth_address: Option<&str>,
    godwoken_rpc_url: &str,
) -> Result<(), String> {
    let deployment_result_string =
        std::fs::read_to_string(deployment_results_path).map_err(|err| err.to_string())?;
    let deployment_result: ScriptsDeploymentResult =
        serde_json::from_str(&deployment_result_string).map_err(|err| err.to_string())?;

    let config = read_config(&config_path)?;

    let privkey_string = fs::read_to_string(privkey_path)
        .map_err(|err| err.to_string())?
        .split_whitespace()
        .next()
        .map(ToOwned::to_owned)
        .ok_or_else(|| "Privkey file is empty".to_string())?;

    // Using private key to calculate eth address when eth_address not provided.
    let eth_address_bytes = match eth_address {
        Some(addr) => {
            let mut b: [u8; 20] = [0u8; 20];
            b.copy_from_slice(&addr.as_bytes()[2..]);
            CKBBytes::from(b.to_vec())
        }
        None => {
            let eth_address = privkey_to_eth_address(privkey_string.as_str())?;
            eth_address
        }
    };
    log::info!("eth address: 0x{:#x}", eth_address_bytes);

    let rollup_type_hash = &config.genesis.rollup_type_hash;

    let owner_lock_hash = Byte32::from_slice(privkey_to_lock_hash(&privkey_string)?.as_bytes())
        .map_err(|err| err.to_string())?;

    // build layer2 lock
    let l2_code_hash = &deployment_result.eth_account_lock.script_type_hash;

    let mut l2_args_vec = rollup_type_hash.as_bytes().to_vec();
    l2_args_vec.append(&mut eth_address_bytes.to_vec());
    let l2_lock_args = GwPack::pack(&GwBytes::from(l2_args_vec));

    let l2_lock = Script::new_builder()
        .code_hash(Byte32::from_slice(l2_code_hash.as_bytes()).map_err(|err| err.to_string())?)
        .hash_type(ScriptHashType::Type.into())
        .args(l2_lock_args)
        .build();

    let l2_lock_hash: H256 = {
        let mut hasher = new_blake2b();
        hasher.update(&l2_lock.as_slice());
        let mut hash = [0u8; 32];
        hasher.finalize(&mut hash);
        hash.into()
    };

    let l2_lock_hash_str = format!(
        "0x{}",
        faster_hex::hex_string(l2_lock_hash.as_bytes()).map_err(|err| err.to_string())?
    );
    log::info!("layer2 script hash: {}", l2_lock_hash_str);

    // cancel_timeout default to 2 days
    let deposit_lock_args = DepositLockArgs::new_builder()
        .owner_lock_hash(owner_lock_hash)
        .cancel_timeout(GwPack::pack(&0xc00000000002a300u64))
        .layer2_lock(l2_lock)
        .build();

    let mut l1_lock_args = rollup_type_hash.as_bytes().to_vec();
    l1_lock_args.append(&mut deposit_lock_args.as_bytes().to_vec());

    let deposit_lock_code_hash = &deployment_result.deposit_lock.script_type_hash;

    let mut rpc_client = HttpRpcClient::new(ckb_rpc_url.to_string());
    let network_type = get_network_type(&mut rpc_client)?;
    let address_payload = AddressPayload::new_full_type(
        CKBPack::pack(deposit_lock_code_hash),
        GwBytes::from(l1_lock_args),
    );
    let address: Address = Address::new(network_type, address_payload);

    let mut godwoken_rpc_client = GodwokenRpcClient::new(godwoken_rpc_url);
    let init_balance = get_balance_by_script_hash(&mut godwoken_rpc_client, &l2_lock_hash)?;

    let output = run_cmd(vec![
        "--url",
        rpc_client.url(),
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
    ])?;
    let tx_hash = H256::from_str(&output.trim()[2..]).map_err(|err| err.to_string())?;
    log::info!("tx_hash: {:#x}", tx_hash);

    wait_for_tx(&mut rpc_client, &tx_hash, 180u64)?;

    wait_for_balance_change(
        &mut godwoken_rpc_client,
        &l2_lock_hash,
        init_balance,
        180u64,
    )?;

    Ok(())
}

pub fn privkey_to_eth_address(privkey: &str) -> Result<CKBBytes, String> {
    let privkey_data = H256::from_str(&privkey.trim()[2..]).map_err(|err| err.to_string())?;
    let privkey = secp256k1::SecretKey::from_slice(privkey_data.as_bytes())
        .map_err(|err| format!("Invalid secp256k1 secret key format, error: {}", err))?;
    let pubkey = secp256k1::PublicKey::from_secret_key(&SECP256K1, &privkey);
    let pubkey_hash = {
        let mut hasher = Keccak256::new();
        hasher.update(&pubkey.serialize_uncompressed()[1..]);
        let buf = hasher.finalize();
        let mut pubkey_hash = [0u8; 20];
        pubkey_hash.copy_from_slice(&buf[12..]);
        pubkey_hash
    };
    let s = CKBBytes::from(pubkey_hash.to_vec());
    Ok(s)
}

fn privkey_to_lock_hash(privkey: &str) -> Result<H256, String> {
    let privkey_data = H256::from_str(&privkey.trim()[2..]).map_err(|err| err.to_string())?;
    let privkey = secp256k1::SecretKey::from_slice(privkey_data.as_bytes())
        .map_err(|err| format!("Invalid secp256k1 secret key format, error: {}", err))?;
    let pubkey = secp256k1::PublicKey::from_secret_key(&SECP256K1, &privkey);
    let address_payload = AddressPayload::from_pubkey(&pubkey);

    let lock_hash: H256 = CKBScript::from(&address_payload)
        .calc_script_hash()
        .unpack();
    Ok(lock_hash)
}

// Read config.toml
pub fn read_config<P: AsRef<Path>>(path: P) -> Result<Config, String> {
    let content = fs::read(&path).map_err(|err| err.to_string())?;
    let config = toml::from_slice(&content).map_err(|err| err.to_string())?;
    Ok(config)
}

fn wait_for_balance_change(
    godwoken_rpc_client: &mut GodwokenRpcClient,
    from_script_hash: &H256,
    init_balance: u128,
    timeout_secs: u64,
) -> Result<(), String> {
    let retry_timeout = Duration::from_secs(timeout_secs);
    let start_time = Instant::now();
    while start_time.elapsed() < retry_timeout {
        std::thread::sleep(Duration::from_secs(2));

        let balance = get_balance_by_script_hash(godwoken_rpc_client, from_script_hash)?;
        log::info!(
            "current balance: {}, waiting for {} secs.",
            balance,
            start_time.elapsed().as_secs()
        );

        if balance != init_balance {
            log::info!("deposit success!");
            let account_id = godwoken_rpc_client
                .get_account_id_by_script_hash(from_script_hash.clone())?
                .unwrap();
            log::info!("Your account id: {}", account_id);
            return Ok(());
        }
    }
    Err(format!("Timeout: {:?}", retry_timeout))
}

fn get_balance_by_script_hash(
    godwoken_rpc_client: &mut GodwokenRpcClient,
    script_hash: &H256,
) -> Result<u128, String> {
    let account_id = godwoken_rpc_client.get_account_id_by_script_hash(script_hash.clone())?;
    match account_id {
        Some(id) => {
            let balance = godwoken_rpc_client.get_balance(id, 1)?;
            Ok(balance)
        }
        None => Ok(0u128),
    }
}
