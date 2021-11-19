use crate::account::{privkey_to_eth_address, read_privkey};
use crate::godwoken_rpc::GodwokenRpcClient;
use crate::hasher::CkbHasher;
use crate::types::ScriptsDeploymentResult;
use crate::utils::transaction::{get_network_type, read_config, run_cmd, wait_for_tx};
use anyhow::{anyhow, Result};
use ckb_fixed_hash::H256;
use ckb_jsonrpc_types::JsonBytes;
use ckb_sdk::{Address, AddressPayload, HttpRpcClient, HumanCapacity, SECP256K1};
use ckb_types::{
    bytes::Bytes as CKBBytes, core::ScriptHashType, packed::Script as CKBScript,
    prelude::Builder as CKBBuilder, prelude::Entity as CKBEntity, prelude::Pack as CKBPack,
    prelude::Unpack as CKBUnpack,
};
use gw_types::packed::{CellOutput, CustodianLockArgs};
use gw_types::{
    bytes::Bytes as GwBytes,
    packed::{Byte32, DepositLockArgs, Script},
    prelude::Pack as GwPack,
};
use std::path::Path;
use std::str::FromStr;
use std::time::{Duration, Instant};
use std::u128;

#[allow(clippy::too_many_arguments)]
pub fn deposit_ckb(
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

    let rollup_type_hash = &config.genesis.rollup_type_hash;

    let owner_lock_hash = Byte32::from_slice(privkey_to_lock_hash(&privkey)?.as_bytes())?;

    // build layer2 lock
    let l2_code_hash = &scripts_deployment.eth_account_lock.script_type_hash;

    let mut l2_args_vec = rollup_type_hash.as_bytes().to_vec();
    l2_args_vec.append(&mut eth_address_bytes.to_vec());
    let l2_lock_args = GwPack::pack(&GwBytes::from(l2_args_vec));

    let l2_lock = Script::new_builder()
        .code_hash(Byte32::from_slice(l2_code_hash.as_bytes())?)
        .hash_type(ScriptHashType::Type.into())
        .args(l2_lock_args)
        .build();

    let l2_lock_hash = CkbHasher::new().update(l2_lock.as_slice()).finalize();

    let l2_lock_hash_str = format!("0x{}", faster_hex::hex_string(l2_lock_hash.as_bytes())?);
    log::info!("layer2 script hash: {}", l2_lock_hash_str);

    // cancel_timeout default to 2 days
    let deposit_lock_args = DepositLockArgs::new_builder()
        .owner_lock_hash(owner_lock_hash)
        .cancel_timeout(GwPack::pack(&0xc00000000002a300u64))
        .layer2_lock(l2_lock)
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

    let mut rpc_client = HttpRpcClient::new(ckb_rpc_url.to_string());
    let network_type = get_network_type(&mut rpc_client)?;
    let address_payload = AddressPayload::new_full_type(
        CKBPack::pack(deposit_lock_code_hash),
        GwBytes::from(l1_lock_args),
    );
    let address: Address = Address::new(network_type, address_payload);

    let mut godwoken_rpc_client = GodwokenRpcClient::new(godwoken_rpc_url);

    let short_address = &l2_lock_hash.as_bytes()[..20];
    log::info!("short address: 0x{}", hex::encode(short_address));

    let init_balance =
        get_balance_by_short_address(&mut godwoken_rpc_client, short_address.to_vec())?;

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
    let tx_hash = H256::from_str(output.trim().trim_start_matches("0x"))?;
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

fn privkey_to_lock_hash(privkey: &H256) -> Result<H256> {
    let privkey = secp256k1::SecretKey::from_slice(privkey.as_bytes())?;
    let pubkey = secp256k1::PublicKey::from_secret_key(&SECP256K1, &privkey);
    let address_payload = AddressPayload::from_pubkey(&pubkey);

    let lock_hash: H256 = CKBScript::from(&address_payload)
        .calc_script_hash()
        .unpack();
    Ok(lock_hash)
}

fn wait_for_balance_change(
    godwoken_rpc_client: &mut GodwokenRpcClient,
    from_script_hash: &H256,
    init_balance: u128,
    timeout_secs: u64,
) -> Result<()> {
    let retry_timeout = Duration::from_secs(timeout_secs);
    let start_time = Instant::now();
    while start_time.elapsed() < retry_timeout {
        std::thread::sleep(Duration::from_secs(2));

        let short_address = &from_script_hash.as_bytes()[..20];
        let balance = get_balance_by_short_address(godwoken_rpc_client, short_address.to_vec())?;
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
    Err(anyhow!("Timeout: {:?}", retry_timeout))
}

fn get_balance_by_short_address(
    godwoken_rpc_client: &mut GodwokenRpcClient,
    short_address: Vec<u8>,
) -> Result<u128> {
    let bytes = JsonBytes::from_vec(short_address);
    let balance = godwoken_rpc_client.get_balance(bytes, 1)?;
    Ok(balance)
}

// only for CKB
fn minimal_deposit_capacity(deposit_lock_args: &DepositLockArgs) -> Result<u64> {
    // fixed size, the specific value is not important.
    let dummy_hash = gw_types::core::H256::zero();
    let dummy_block_number = 0u64;
    let dummy_rollup_type_hash = dummy_hash;

    let custodian_lock_args = CustodianLockArgs::new_builder()
        .deposit_block_hash(dummy_hash.pack())
        .deposit_block_number(gw_types::prelude::Pack::pack(&dummy_block_number))
        .deposit_lock_args(deposit_lock_args.clone())
        .build();

    let args: gw_types::bytes::Bytes = dummy_rollup_type_hash
        .as_slice()
        .iter()
        .chain(custodian_lock_args.as_slice().iter())
        .cloned()
        .collect();

    let lock_script = Script::new_builder()
        .code_hash(dummy_hash.pack())
        .hash_type(ScriptHashType::Type.into())
        .args(gw_types::prelude::Pack::pack(&args))
        .build();

    // no type / data when deposit CKB
    let output = CellOutput::new_builder()
        .capacity(gw_types::prelude::Pack::pack(&0))
        .lock(lock_script)
        .build();

    let capacity = output.occupied_capacity(0)?;
    Ok(capacity)
}

fn parse_capacity(capacity: &str) -> Result<u64> {
    let human_capacity = HumanCapacity::from_str(capacity).map_err(|err| anyhow!(err))?;
    Ok(human_capacity.into())
}
