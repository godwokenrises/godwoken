use std::{
    path::Path,
    str::FromStr,
    time::{Duration, Instant},
};

use anyhow::{anyhow, bail, Result};
use ckb_fixed_hash::H256;
use ckb_types::core::Capacity;
use gw_common::builtins::{CKB_SUDT_ACCOUNT_ID, ETH_REGISTRY_ACCOUNT_ID};
use gw_types::{
    core::{ScriptHashType, Timepoint},
    packed::{CellOutput, CustodianLockArgs, DepositLockArgs, Script},
    prelude::*,
    U256,
};
use gw_utils::transaction_skeleton::TransactionSkeleton;

use crate::{
    account::{privkey_to_eth_address, read_privkey},
    godwoken_rpc::GodwokenRpcClient,
    types::ScriptsDeploymentResult,
    utils::{deploy::DeployContextArgs, sdk::HumanCapacity, transaction::read_config},
};

#[allow(clippy::too_many_arguments)]
pub async fn deposit_ckb(
    privkey_path: &Path,
    scripts_deployment_path: &Path,
    config_path: &Path,
    capacity: &str,
    // TODO: setting fee.
    _fee: &str,
    ckb_rpc_url: &str,
    ckb_indexer_rpc_url: Option<&str>,
    eth_address: Option<&str>,
    godwoken_rpc_url: &str,
) -> Result<()> {
    let scripts_deployment_content = std::fs::read_to_string(scripts_deployment_path)?;
    let scripts_deployment: ScriptsDeploymentResult =
        serde_json::from_str(&scripts_deployment_content)?;

    let config = read_config(&config_path)?;

    let context = DeployContextArgs {
        ckb_rpc: ckb_rpc_url.into(),
        ckb_indexer_rpc: ckb_indexer_rpc_url.map(Into::into),
        privkey_path: privkey_path.into(),
    }
    .build()
    .await?;

    let privkey = read_privkey(privkey_path)?;

    // Using private key to calculate eth address when eth_address not provided.
    let eth_address_bytes = match eth_address {
        Some(addr) => hex::decode(&addr.trim_start_matches("0x").as_bytes())?.into(),
        None => privkey_to_eth_address(&privkey)?,
    };
    log::info!("eth address: 0x{:#x}", eth_address_bytes);

    let rollup_type_hash = &config.consensus.get_config().genesis.rollup_type_hash;

    let owner_lock_hash = context.wallet.lock_script().hash();

    // build layer2 lock
    let l2_code_hash = &scripts_deployment.eth_account_lock.script_type_hash;

    let mut l2_args = rollup_type_hash.as_bytes().to_vec();
    l2_args.append(&mut eth_address_bytes.to_vec());

    let l2_lock = Script::new_builder()
        .code_hash(l2_code_hash.pack())
        .hash_type(ScriptHashType::Type.into())
        .args(l2_args.pack())
        .build();

    let l2_lock_hash: H256 = l2_lock.hash().into();

    log::info!("layer2 script hash: 0x{}", l2_lock_hash);

    // cancel_timeout default to 20 minutes
    let deposit_lock_args = DepositLockArgs::new_builder()
        .owner_lock_hash(owner_lock_hash.pack())
        .cancel_timeout(0xc0000000000004b0u64.pack())
        .layer2_lock(l2_lock)
        .registry_id(ETH_REGISTRY_ACCOUNT_ID.pack())
        .build();

    let minimal_capacity = minimal_deposit_capacity(&deposit_lock_args)?;
    let capacity_in_shannons = parse_capacity(capacity)?;
    if capacity_in_shannons < minimal_capacity {
        bail!(
            "Deposit CKB required {} CKB at least, provided {}.",
            HumanCapacity::from(minimal_capacity).to_string(),
            HumanCapacity::from(capacity_in_shannons).to_string()
        );
    }

    let mut l1_lock_args = rollup_type_hash.as_bytes().to_vec();
    l1_lock_args.append(&mut deposit_lock_args.as_bytes().to_vec());

    let deposit_lock_code_hash = &scripts_deployment.deposit_lock.script_type_hash;

    let deposit_lock = Script::new_builder()
        .code_hash(deposit_lock_code_hash.pack())
        .hash_type(ScriptHashType::Type.into())
        .args(l1_lock_args.pack())
        .build();

    let mut godwoken_rpc_client = GodwokenRpcClient::new(godwoken_rpc_url);

    let init_balance = get_balance_by_script_hash(&mut godwoken_rpc_client, &l2_lock_hash).await?;
    log::info!("balance before deposit: {}", init_balance);

    let mut tx = TransactionSkeleton::new([0u8; 32]);
    tx.transfer_to(deposit_lock, capacity_in_shannons)?;
    let tx = context.deploy(tx, &Default::default()).await?;

    let tx_hash: H256 = tx.hash().into();
    log::info!("Sent transaction 0x{tx_hash}");
    context
        .ckb_client
        .wait_tx_committed_with_timeout_and_logging(tx_hash.0, 600)
        .await?;

    wait_for_balance_change(
        &mut godwoken_rpc_client,
        &l2_lock_hash,
        init_balance,
        180u64,
    )
    .await?;

    Ok(())
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
