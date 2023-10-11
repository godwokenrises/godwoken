use std::{
    collections::HashSet,
    iter::FromIterator,
    ops::Sub,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{anyhow, ensure, Context, Result};
use ckb_fixed_hash::H256;
use ckb_resource::CODE_HASH_SECP256K1_DATA;
use gw_config::GenesisConfig;
use gw_generator::genesis::build_genesis;
use gw_jsonrpc_types::JsonCalcHash;
use gw_rpc_client::ckb_client::CkbClient;
use gw_types::{
    bytes::Bytes,
    core::{AllowedContractType, AllowedEoaType, DepType, ScriptHashType},
    packed,
    prelude::*,
};
use gw_utils::{
    fee::collect_payment_cells,
    local_cells::LocalCellsManager,
    transaction_skeleton::TransactionSkeleton,
    type_id::{type_id_args, type_id_type_script},
};

use crate::{
    types::{RollupDeploymentResult, ScriptsDeploymentResult, UserRollupConfig},
    utils::deploy::DeployContextArgs,
};

pub struct DeployRollupCellArgs<'a> {
    pub privkey_path: &'a Path,
    pub ckb_rpc_url: &'a str,
    pub ckb_indexer_rpc_url: Option<&'a str>,
    pub scripts_result: &'a ScriptsDeploymentResult,
    pub user_rollup_config: &'a UserRollupConfig,
    pub timestamp: Option<u64>,
    pub skip_config_check: bool,
}

pub async fn deploy_rollup_cell(args: DeployRollupCellArgs<'_>) -> Result<RollupDeploymentResult> {
    let DeployRollupCellArgs {
        privkey_path,
        ckb_rpc_url,
        ckb_indexer_rpc_url,
        scripts_result,
        user_rollup_config,
        timestamp,
        skip_config_check,
    } = args;
    let (r, u) = (scripts_result, user_rollup_config);

    let burn_lock_hash: H256 = user_rollup_config.burn_lock.hash();
    // check config
    if !skip_config_check {
        let expected_burn_lock_script = packed::Script::new_builder()
            .code_hash([0u8; 32].pack())
            .hash_type(ScriptHashType::Data.into())
            .build();
        let expected_burn_lock_hash: H256 = expected_burn_lock_script.calc_script_hash().unpack();
        if expected_burn_lock_hash != burn_lock_hash {
            return Err(anyhow!(
                "The burn lock hash is not default, we suggest to use default burn lock \
                (code_hash: 0x, hash_type: Data, args: empty)",
            ));
        }
    }

    let context = DeployContextArgs {
        ckb_rpc: ckb_rpc_url.into(),
        ckb_indexer_rpc: ckb_indexer_rpc_url.map(Into::into),
        privkey_path: privkey_path.into(),
    }
    .build()
    .await?;

    // deploy rollup config cell
    let allowed_contract_type_hashes: Vec<packed::AllowedTypeHash> = {
        let meta = packed::AllowedTypeHash::new_builder()
            .type_(AllowedContractType::Meta.into())
            .hash(r.meta_contract_validator.script_type_hash.pack())
            .build();
        let sudt = packed::AllowedTypeHash::new_builder()
            .type_(AllowedContractType::Sudt.into())
            .hash(r.l2_sudt_validator.script_type_hash.pack())
            .build();
        let polyjuice = packed::AllowedTypeHash::new_builder()
            .type_(AllowedContractType::Polyjuice.into())
            .hash(r.polyjuice_validator.script_type_hash.pack())
            .build();
        let eth_addr_reg_validator = packed::AllowedTypeHash::new_builder()
            .type_(AllowedContractType::EthAddrReg.into())
            .hash(r.eth_addr_reg_validator.script_type_hash.pack())
            .build();

        let mut type_hashes = vec![meta, sudt, polyjuice, eth_addr_reg_validator];
        let builtin_hashes = [
            &scripts_result.meta_contract_validator.script_type_hash,
            &scripts_result.l2_sudt_validator.script_type_hash,
            &scripts_result.polyjuice_validator.script_type_hash,
            &scripts_result.eth_addr_reg_validator.script_type_hash,
        ];

        let user_hashes: HashSet<_> =
            HashSet::from_iter(&user_rollup_config.allowed_contract_type_hashes);
        for user_hash in user_hashes {
            if builtin_hashes.contains(&user_hash) {
                continue;
            }

            type_hashes.push(packed::AllowedTypeHash::from_unknown(user_hash.0));
        }
        type_hashes
    };

    // EOA scripts
    let allowed_eoa_type_hashes: Vec<packed::AllowedTypeHash> = {
        let eth_hash = scripts_result.eth_account_lock.script_type_hash.pack();
        let eth = packed::AllowedTypeHash::new_builder()
            .type_(AllowedEoaType::Eth.into())
            .hash(eth_hash)
            .build();

        let mut type_hashes = vec![eth];
        let builtin_hashes = [&scripts_result.eth_account_lock.script_type_hash];

        let user_hashes: HashSet<_> =
            HashSet::from_iter(&user_rollup_config.allowed_eoa_type_hashes);
        for user_hash in user_hashes {
            if builtin_hashes.contains(&user_hash) {
                continue;
            }

            type_hashes.push(packed::AllowedTypeHash::from_unknown(user_hash.0));
        }
        type_hashes
    };

    // composite rollup config
    let rollup_config = packed::RollupConfig::new_builder()
        .l1_sudt_script_type_hash(u.l1_sudt_script_type_hash.pack())
        .custodian_script_type_hash(r.custodian_lock.script_type_hash.pack())
        .deposit_script_type_hash(r.deposit_lock.script_type_hash.pack())
        .withdrawal_script_type_hash(r.withdrawal_lock.script_type_hash.pack())
        .challenge_script_type_hash(r.challenge_lock.script_type_hash.pack())
        .stake_script_type_hash(r.stake_lock.script_type_hash.pack())
        .l2_sudt_validator_script_type_hash(r.l2_sudt_validator.script_type_hash.pack())
        .burn_lock_hash(burn_lock_hash.pack())
        .required_staking_capacity(u.required_staking_capacity.pack())
        .challenge_maturity_blocks(u.challenge_maturity_blocks.pack())
        .finality_blocks(u.finality_blocks.pack())
        .reward_burn_rate(u.reward_burn_rate.into())
        .chain_id(u.chain_id.pack())
        .allowed_eoa_type_hashes(PackVec::pack(allowed_eoa_type_hashes))
        .allowed_contract_type_hashes(PackVec::pack(allowed_contract_type_hashes))
        .build();

    let (secp_data, secp_data_dep) = get_secp_data(&context.ckb_client).await?;

    let mut local_cells = LocalCellsManager::default();

    let mut tx = TransactionSkeleton::new([0u8; 32]);
    tx.add_output(
        user_rollup_config.cells_lock.clone().into(),
        None,
        rollup_config.as_bytes(),
    )?;
    let tx = context
        .deploy(tx, &local_cells)
        .await
        .context("deploy rollup config cell")?;
    let tx_hash: H256 = tx.hash().into();
    log::info!("Sent transaction {} to deploy rollup config cell", tx_hash);
    local_cells.apply_tx(&tx.as_reader());

    let rollup_config_cell_dep = {
        let out_point = packed::OutPoint::new_builder()
            .tx_hash(tx_hash.pack())
            .index(0u32.pack())
            .build();

        packed::CellDep::new_builder()
            .out_point(out_point)
            .dep_type(DepType::Code.into())
            .build()
    };
    let rollup_config_tx_hash = tx_hash;

    // millisecond
    let timestamp = timestamp.unwrap_or_else(|| {
        // New created CKB dev chain's may out of sync with real world time,
        // So we using an earlier time to get around this issue.
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("timestamp")
            .sub(core::time::Duration::from_secs(3600))
            .as_millis() as u64
    });

    let mut tx = TransactionSkeleton::new([0u8; 32]);

    // Collect at least one payment cell for type-id calculation.
    let payment_cells = collect_payment_cells(
        &context.ckb_indexer_client,
        context.wallet.lock_script().clone(),
        1,
        &Default::default(),
        &local_cells,
    )
    .await?;
    ensure!(!payment_cells.is_empty(), "no live payment cells");
    tx.inputs_mut()
        .extend(payment_cells.into_iter().map(Into::into));

    let first_input = tx.inputs()[0].input.clone();

    // Delegate cell.
    let delegate_cell_data = context
        .wallet
        .lock_script()
        .calc_script_hash()
        .as_bytes()
        .slice(..20);
    let delegate_cell_type_script = type_id_type_script(first_input.as_reader(), 0);

    assert_eq!(tx.outputs().len(), 0);
    tx.add_output(
        u.cells_lock.clone().into(),
        Some(delegate_cell_type_script.clone()),
        delegate_cell_data,
    )?;

    // Rollup cell type script.
    let rollup_cell_type_id_args = type_id_args(first_input.as_reader(), 1);
    let rollup_type_script = packed::Script::new_builder()
        .code_hash(r.state_validator.script_type_hash.pack())
        .hash_type(ScriptHashType::Type.into())
        .args(rollup_cell_type_id_args.pack())
        .build();
    let rollup_script_hash: H256 = rollup_type_script.calc_script_hash().unpack();
    log::info!("rollup_script_hash: {:#x}", rollup_script_hash);

    // 1. build genesis block
    let genesis_config = GenesisConfig {
        timestamp,
        meta_contract_validator_type_hash: scripts_result
            .meta_contract_validator
            .script_type_hash
            .clone(),
        eth_registry_validator_type_hash: scripts_result
            .eth_addr_reg_validator
            .script_type_hash
            .clone(),
        rollup_type_hash: rollup_script_hash.clone(),
        rollup_config: rollup_config.clone().into(),
        secp_data_dep,
    };
    let genesis_with_global_state = build_genesis(&genesis_config, secp_data)?;

    // 2. build rollup cell
    {
        let data = genesis_with_global_state.global_state.as_bytes();
        // Use delegate-cell-lock.
        let lock = {
            packed::Script::new_builder()
                .code_hash(r.delegate_cell_lock.script_type_hash.pack())
                .hash_type(ScriptHashType::Type.into())
                // Delegate cell type script hash.
                .args(
                    delegate_cell_type_script
                        .calc_script_hash()
                        .as_bytes()
                        .pack(),
                )
                .build()
        };

        assert_eq!(tx.outputs().len(), 1);
        tx.add_output(lock, Some(rollup_type_script.clone()), data)?;
    };

    // 3. put genesis block in rollup cell witness
    let witness_0 = {
        let output_type = genesis_with_global_state.genesis.as_bytes();
        packed::WitnessArgs::new_builder()
            .output_type(Some(output_type).pack())
            .build()
    };
    assert_eq!(tx.witnesses().len(), 0);
    tx.witnesses_mut().push(witness_0);

    // Special cell deps: rollup config, state validator.
    tx.cell_deps_mut().extend([
        rollup_config_cell_dep.clone(),
        r.state_validator.cell_dep.clone().into(),
    ]);

    let tx = context
        .deploy(tx, &local_cells)
        .await
        .context("deploy genesis cell")?;
    let tx_hash: H256 = tx.hash().into();
    log::info!("Sent tx {} to deploy genesis and delegate cell", tx_hash);

    context
        .ckb_client
        .wait_tx_committed_with_timeout_and_logging(rollup_config_tx_hash.0, 180)
        .await?;
    context
        .ckb_client
        .wait_tx_committed_with_timeout_and_logging(tx_hash.0, 180)
        .await?;

    // 5. write genesis deployment result
    let rollup_result = RollupDeploymentResult {
        tx_hash,
        timestamp,
        rollup_type_hash: rollup_script_hash,
        rollup_type_script: rollup_type_script.into(),
        rollup_config: rollup_config.into(),
        rollup_config_cell_dep: rollup_config_cell_dep.into(),
        delegate_cell_type_script: delegate_cell_type_script.into(),
        genesis_config,
        layer2_genesis_hash: genesis_with_global_state.genesis.hash().into(),
    };
    Ok(rollup_result)
}

pub async fn get_secp_data(
    rpc_client: &CkbClient,
) -> Result<(Bytes, gw_jsonrpc_types::ckb_jsonrpc_types::CellDep)> {
    let mut cell_dep = None;
    rpc_client
        .get_block_by_number(0.into())
        .await?
        .context("get CKB genesis block")?
        .transactions
        .iter()
        .for_each(|tx| {
            tx.inner
                .outputs_data
                .iter()
                .enumerate()
                .for_each(|(output_index, data)| {
                    let data_hash = ckb_types::packed::CellOutput::calc_data_hash(data.as_bytes());
                    if data_hash.as_slice() == CODE_HASH_SECP256K1_DATA.as_bytes() {
                        let out_point = gw_jsonrpc_types::blockchain::OutPoint {
                            tx_hash: tx.hash.clone(),
                            index: (output_index as u32).into(),
                        };
                        cell_dep = Some((
                            data.clone().into_bytes(),
                            gw_jsonrpc_types::blockchain::CellDep {
                                out_point,
                                dep_type: gw_jsonrpc_types::blockchain::DepType::Code,
                            },
                        ));
                    }
                });
        });
    cell_dep.ok_or_else(|| anyhow!("Can not found secp data cell in CKB genesis block"))
}
