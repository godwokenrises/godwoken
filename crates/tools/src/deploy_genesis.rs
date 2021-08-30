use std::path::Path;
use std::str::FromStr;
use std::{fs, ops::Deref};

use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;

use ckb_fixed_hash::H256;
use ckb_hash::new_blake2b;
use ckb_jsonrpc_types as rpc_types;
use ckb_resource::CODE_HASH_SECP256K1_DATA;
use ckb_sdk::{
    calc_max_mature_number,
    constants::{CELLBASE_MATURITY, MIN_SECP_CELL_CAPACITY, ONE_CKB},
    Address, AddressPayload, GenesisInfo, HttpRpcClient, HumanCapacity, SECP256K1,
};
use ckb_types::{
    bytes::{Bytes, BytesMut},
    core::{
        BlockView, Capacity, DepType, EpochNumberWithFraction, ScriptHashType, TransactionBuilder,
        TransactionView,
    },
    packed as ckb_packed,
    prelude::Builder as CKBBuilder,
    prelude::Pack as CKBPack,
    prelude::Unpack as CKBUnpack,
};
use gw_config::GenesisConfig;
use gw_generator::genesis::build_genesis;
use gw_types::{
    packed as gw_packed, packed::RollupConfig, prelude::Entity as GwEntity,
    prelude::Pack as GwPack, prelude::PackVec as GwPackVec,
};

use super::deploy_scripts::ScriptsDeploymentResult;
use crate::utils::{get_network_type, run_cmd, wait_for_tx, TYPE_ID_CODE_HASH};

use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug, Default)]
pub struct UserRollupConfig {
    pub l1_sudt_script_type_hash: H256,
    pub burn_lock_hash: H256,
    pub required_staking_capacity: u64,
    pub challenge_maturity_blocks: u64,
    pub finality_blocks: u64,
    pub reward_burn_rate: u8, // * reward_burn_rate / 100
    pub allowed_eoa_type_hashes: Vec<H256>,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug, Default)]
pub struct PoAConfig {
    pub poa_setup: PoASetup,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug, Default)]
pub struct PoASetup {
    pub identity_size: u8,
    pub round_interval_uses_seconds: bool,
    pub identities: Vec<rpc_types::JsonBytes>,
    pub aggregator_change_threshold: u8,
    pub round_intervals: u32,
    pub subblocks_per_round: u32,
}

pub fn serialize_poa_setup(setup: &PoASetup) -> Bytes {
    let mut buffer = BytesMut::new();
    if setup.round_interval_uses_seconds {
        buffer.extend_from_slice(&[1]);
    } else {
        buffer.extend_from_slice(&[0]);
    }
    if setup.identities.len() > 255 {
        panic!("Too many identities!");
    }
    buffer.extend_from_slice(&[
        setup.identity_size,
        setup.identities.len() as u8,
        setup.aggregator_change_threshold,
    ]);
    buffer.extend_from_slice(&setup.round_intervals.to_le_bytes()[..]);
    buffer.extend_from_slice(&setup.subblocks_per_round.to_le_bytes()[..]);
    for identity in &setup.identities {
        if identity.len() < setup.identity_size as usize {
            panic!("Invalid identity!");
        }
        buffer.extend_from_slice(&identity.as_bytes()[..setup.identity_size as usize]);
    }
    buffer.freeze()
}

pub struct PoAData {
    pub round_initial_subtime: u64,
    pub subblock_subtime: u64,
    pub subblock_index: u32,
    pub aggregator_index: u16,
}

pub fn serialize_poa_data(data: &PoAData) -> Bytes {
    let mut buffer = BytesMut::new();
    buffer.extend_from_slice(&data.round_initial_subtime.to_le_bytes()[..]);
    buffer.extend_from_slice(&data.subblock_subtime.to_le_bytes()[..]);
    buffer.extend_from_slice(&data.subblock_index.to_le_bytes()[..]);
    buffer.extend_from_slice(&data.aggregator_index.to_le_bytes()[..]);
    buffer.freeze()
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Debug, Default)]
pub struct GenesisDeploymentResult {
    pub tx_hash: H256,
    pub timestamp: u64,
    pub rollup_type_hash: H256,
    pub rollup_type_script: ckb_jsonrpc_types::Script,
    pub rollup_config: gw_jsonrpc_types::godwoken::RollupConfig,
    pub rollup_config_cell_dep: ckb_jsonrpc_types::CellDep,
    pub layer2_genesis_hash: H256,
    pub genesis_config: GenesisConfig,
}

struct DeployContext<'a> {
    privkey_path: &'a Path,
    owner_address: &'a Address,
    genesis_info: &'a GenesisInfo,
    deployment_result: &'a ScriptsDeploymentResult,
}

impl<'a> DeployContext<'a> {
    fn deploy(
        &mut self,
        rpc_client: &mut HttpRpcClient,
        mut outputs: Vec<ckb_packed::CellOutput>,
        mut outputs_data: Vec<Bytes>,
        mut deps: Vec<ckb_packed::CellDep>,
        first_cell_input: Option<&ckb_packed::CellInput>,
        witness_0: ckb_packed::WitnessArgs,
    ) -> Result<H256, String> {
        let tx_fee = ONE_CKB;
        let total_output_capacity: u64 = outputs
            .iter()
            .map(|output| {
                let value: u64 = CKBUnpack::unpack(&output.capacity());
                value
            })
            .sum();
        let total_capacity = total_output_capacity + tx_fee;
        let tip_number = rpc_client.get_tip_block_number()?;
        let max_mature_number = get_max_mature_number(rpc_client)?;
        let (inputs, total_input_capacity) = collect_live_cells(
            rpc_client.url(),
            self.owner_address.to_string().as_str(),
            max_mature_number,
            tip_number,
            total_capacity,
        )?;
        if let Some(first_input) = first_cell_input {
            if inputs[0].as_slice() != first_input.as_slice() {
                return Err("first input cell changed".to_string());
            }
        }
        // collect_live_cells will ensure `total_input_capacity >= total_capacity`.
        let rest_capacity = total_input_capacity - total_capacity;
        let max_tx_fee_str = if rest_capacity >= MIN_SECP_CELL_CAPACITY {
            outputs.push(
                ckb_packed::CellOutput::new_builder()
                    .lock(ckb_packed::Script::from(self.owner_address.payload()))
                    .capacity(CKBPack::pack(&rest_capacity))
                    .build(),
            );
            outputs_data.push(Default::default());
            "1.0"
        } else {
            "62.0"
        };
        let outputs_data: Vec<ckb_packed::Bytes> = outputs_data
            .iter()
            .map(|data| CKBPack::pack(data))
            .collect();
        deps.extend_from_slice(&[
            self.deployment_result
                .state_validator
                .cell_dep
                .clone()
                .into(),
            self.genesis_info.sighash_dep(),
        ]);
        let tx: TransactionView = TransactionBuilder::default()
            .cell_deps(deps)
            .set_inputs(inputs)
            .set_outputs(outputs)
            .set_outputs_data(outputs_data)
            .set_witnesses(vec![CKBPack::pack(&witness_0.as_bytes())])
            .build();

        // 7. build ckb-cli tx and sign
        let tx_file = NamedTempFile::new().map_err(|err| err.to_string())?;
        let tx_path_str = tx_file.path().to_str().unwrap();
        let _output = run_cmd(&[
            "--url",
            rpc_client.url(),
            "tx",
            "init",
            "--tx-file",
            tx_path_str,
        ])?;
        let tx_json = rpc_types::Transaction::from(tx.data());
        let tx_body: serde_json::Value = serde_json::to_value(&tx_json).unwrap();
        let cli_tx_content = std::fs::read_to_string(tx_path_str).unwrap();
        let mut cli_tx: serde_json::Value = serde_json::from_str(&cli_tx_content).unwrap();
        cli_tx["transaction"] = tx_body;
        let cli_tx_content = serde_json::to_string_pretty(&cli_tx).unwrap();
        std::fs::write(tx_path_str, cli_tx_content.as_bytes()).map_err(|err| err.to_string())?;
        let _output = run_cmd(&[
            "--url",
            rpc_client.url(),
            "tx",
            "sign-inputs",
            "--privkey-path",
            self.privkey_path.to_str().expect("non-utf8 file path"),
            "--tx-file",
            tx_path_str,
            "--add-signatures",
        ])?;

        // 8. send and then wait for tx
        let send_output = run_cmd(&[
            "--url",
            rpc_client.url(),
            "tx",
            "send",
            "--tx-file",
            tx_path_str,
            "--max-tx-fee",
            max_tx_fee_str,
            "--skip-check",
        ])?;
        let tx_hash = H256::from_str(&send_output.trim()[2..]).map_err(|err| err.to_string())?;
        log::info!("tx_hash: {:#x}", tx_hash);
        wait_for_tx(rpc_client, &tx_hash, 120)?;
        Ok(tx_hash)
    }
}

#[allow(clippy::too_many_arguments)]
pub fn deploy_genesis(
    privkey_path: &Path,
    ckb_rpc_url: &str,
    deployment_result_path: &Path,
    user_rollup_config_path: &Path,
    poa_config_path: &Path,
    timestamp: Option<u64>,
    output_path: &Path,
    skip_config_check: bool,
) -> Result<(), String> {
    let deployment_result_string =
        std::fs::read_to_string(deployment_result_path).map_err(|err| err.to_string())?;
    let deployment_result: ScriptsDeploymentResult =
        serde_json::from_str(&deployment_result_string).map_err(|err| err.to_string())?;
    let user_rollup_config_string =
        std::fs::read_to_string(user_rollup_config_path).map_err(|err| err.to_string())?;
    let user_rollup_config: UserRollupConfig =
        serde_json::from_str(&user_rollup_config_string).map_err(|err| err.to_string())?;
    let poa_config_string =
        std::fs::read_to_string(poa_config_path).map_err(|err| err.to_string())?;
    let poa_config: PoAConfig =
        serde_json::from_str(&poa_config_string).map_err(|err| err.to_string())?;
    let poa_setup = poa_config.poa_setup;

    if !skip_config_check {
        let burn_lock_script = ckb_packed::Script::new_builder()
            .code_hash(CKBPack::pack(&[0u8; 32]))
            .hash_type(ScriptHashType::Data.into())
            .build();
        let burn_lock_script_hash: [u8; 32] = burn_lock_script.calc_script_hash().unpack();
        if H256(burn_lock_script_hash) != user_rollup_config.burn_lock_hash {
            return Err(format!(
                "The burn lock hash: 0x{} is not default, we suggest to use default burn lock \
                0x{} (code_hash: 0x, hash_type: Data, args: empty)",
                hex::encode(user_rollup_config.burn_lock_hash),
                hex::encode(burn_lock_script_hash)
            ));
        }
        if poa_setup.round_intervals == 0 {
            return Err("round intervals value must be greater than 0".to_string());
        }
    }

    let mut rpc_client = HttpRpcClient::new(ckb_rpc_url.to_string());
    let network_type = get_network_type(&mut rpc_client)?;
    let privkey_string = std::fs::read_to_string(privkey_path)
        .map_err(|err| err.to_string())?
        .split_whitespace()
        .next()
        .map(ToOwned::to_owned)
        .ok_or_else(|| "File is empty".to_string())?;
    let privkey_data =
        H256::from_str(&privkey_string.trim()[2..]).map_err(|err| err.to_string())?;
    let privkey = secp256k1::SecretKey::from_slice(privkey_data.as_bytes())
        .map_err(|err| format!("Invalid secp256k1 secret key format, error: {}", err))?;
    let pubkey = secp256k1::PublicKey::from_secret_key(&SECP256K1, &privkey);
    let owner_address_payload = AddressPayload::from_pubkey(&pubkey);
    let owner_address = Address::new(network_type, owner_address_payload);
    let owner_address_string = owner_address.to_string();
    let max_mature_number = get_max_mature_number(&mut rpc_client)?;
    let genesis_block: BlockView = rpc_client
        .get_block_by_number(0)?
        .expect("Can not get genesis block?")
        .into();
    let genesis_info = GenesisInfo::from_block(&genesis_block)?;

    // deploy rollup config cell
    let allowed_contract_type_hashes: Vec<gw_packed::Byte32> = vec![
        GwPack::pack(&deployment_result.meta_contract_validator.script_type_hash),
        GwPack::pack(&deployment_result.l2_sudt_validator.script_type_hash),
        GwPack::pack(&deployment_result.polyjuice_validator.script_type_hash),
    ];

    let mut allowed_eoa_type_hashes: Vec<gw_packed::Byte32> = vec![GwPack::pack(
        &deployment_result.eth_account_lock.script_type_hash,
    )];
    allowed_eoa_type_hashes.extend(
        user_rollup_config
            .allowed_eoa_type_hashes
            .into_iter()
            .map(|hash| GwPack::pack(&hash)),
    );
    allowed_eoa_type_hashes.dedup();
    let rollup_config = RollupConfig::new_builder()
        .l1_sudt_script_type_hash(GwPack::pack(&user_rollup_config.l1_sudt_script_type_hash))
        .custodian_script_type_hash(GwPack::pack(
            &deployment_result.custodian_lock.script_type_hash,
        ))
        .deposit_script_type_hash(GwPack::pack(
            &deployment_result.deposit_lock.script_type_hash,
        ))
        .withdrawal_script_type_hash(GwPack::pack(
            &deployment_result.withdrawal_lock.script_type_hash,
        ))
        .challenge_script_type_hash(GwPack::pack(
            &deployment_result.challenge_lock.script_type_hash,
        ))
        .stake_script_type_hash(GwPack::pack(&deployment_result.stake_lock.script_type_hash))
        .l2_sudt_validator_script_type_hash(GwPack::pack(
            &deployment_result.l2_sudt_validator.script_type_hash,
        ))
        .burn_lock_hash(GwPack::pack(&user_rollup_config.burn_lock_hash))
        .required_staking_capacity(GwPack::pack(&user_rollup_config.required_staking_capacity))
        .challenge_maturity_blocks(GwPack::pack(&user_rollup_config.challenge_maturity_blocks))
        .finality_blocks(GwPack::pack(&user_rollup_config.finality_blocks))
        .reward_burn_rate(user_rollup_config.reward_burn_rate.into())
        .allowed_eoa_type_hashes(GwPackVec::pack(allowed_eoa_type_hashes))
        .allowed_contract_type_hashes(GwPackVec::pack(allowed_contract_type_hashes))
        .build();
    let (secp_data, secp_data_dep) = get_secp_data(&mut rpc_client)?;
    let mut deploy_context = DeployContext {
        privkey_path,
        owner_address: &owner_address,
        genesis_info: &genesis_info,
        deployment_result: &deployment_result,
    };
    // FIXME: Right now, to update a rollup config, deploy an new one.
    let (rollup_config_output, rollup_config_data): (ckb_packed::CellOutput, Bytes) = {
        let data = rollup_config.as_bytes();
        let lock_script = ckb_packed::Script::new_builder()
            .code_hash(CKBPack::pack(&[0u8; 32]))
            .hash_type(ScriptHashType::Data.into())
            .build();
        let output = ckb_packed::CellOutput::new_builder()
            .lock(lock_script)
            .build();
        let output = fit_output_capacity(output, data.len());
        (output, data)
    };
    let rollup_config_cell_dep = {
        let tx_hash = deploy_context.deploy(
            &mut rpc_client,
            vec![rollup_config_output],
            vec![rollup_config_data],
            Default::default(),
            None,
            Default::default(),
        )?;
        let out_point = ckb_packed::OutPoint::new_builder()
            .tx_hash(CKBPack::pack(&tx_hash))
            .index(CKBPack::pack(&0u32))
            .build();

        ckb_packed::CellDep::new_builder()
            .out_point(out_point)
            .dep_type(DepType::Code.into())
            .build()
    };

    // millisecond
    let timestamp = timestamp.unwrap_or_else(|| {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("timestamp")
            .as_millis() as u64
    });

    let first_cell_input: ckb_packed::CellInput = get_live_cells(
        rpc_client.url(),
        owner_address_string.as_str(),
        max_mature_number,
        None,
        None,
        Some(1),
    )?
    .into_iter()
    .next()
    .map(|(input, _)| input)
    .ok_or_else(|| format!("No live cell found for address: {}", owner_address_string))?;

    let rollup_cell_type_id: Bytes = calculate_type_id(&first_cell_input, 0);
    let poa_setup_cell_type_id: Bytes = calculate_type_id(&first_cell_input, 1);
    let poa_data_cell_type_id: Bytes = calculate_type_id(&first_cell_input, 2);
    // calculate by: blake2b_hash(firstInput + rullupCell.outputIndex)
    let rollup_type_script = ckb_packed::Script::new_builder()
        .code_hash(CKBPack::pack(
            &deployment_result.state_validator.script_type_hash,
        ))
        .hash_type(ScriptHashType::Type.into())
        .args(CKBPack::pack(&rollup_cell_type_id))
        .build();
    let rollup_script_hash: H256 = CKBUnpack::unpack(&rollup_type_script.calc_script_hash());
    log::info!("rollup_script_hash: {:#x}", rollup_script_hash);

    // 1. build genesis block
    let genesis_config = GenesisConfig {
        timestamp,
        meta_contract_validator_type_hash: deployment_result
            .meta_contract_validator
            .script_type_hash
            .clone(),
        rollup_type_hash: rollup_script_hash.clone(),
        rollup_config: rollup_config.clone().into(),
        secp_data_dep,
    };
    let genesis_with_global_state =
        build_genesis(&genesis_config, secp_data).map_err(|err| err.to_string())?;

    // 2. build rollup cell (with type id)
    let (rollup_output, rollup_data): (ckb_packed::CellOutput, Bytes) = {
        let data = genesis_with_global_state.global_state.as_bytes();
        let lock_args = Bytes::from(
            [
                poa_setup_cell_type_id.deref(),
                poa_data_cell_type_id.deref(),
            ]
            .concat()
            .to_vec(),
        );
        let lock_script = ckb_packed::Script::new_builder()
            .code_hash(CKBPack::pack(
                &deployment_result.state_validator_lock.script_type_hash,
            ))
            .hash_type(ScriptHashType::Type.into())
            .args(CKBPack::pack(&lock_args))
            .build();
        let output = ckb_packed::CellOutput::new_builder()
            .lock(lock_script)
            .type_(CKBPack::pack(&Some(rollup_type_script.clone())))
            .build();
        let output = fit_output_capacity(output, data.len());
        (output, data)
    };

    // 3. build PoA setup cell (with type id)
    let (poa_setup_output, poa_setup_data): (ckb_packed::CellOutput, Bytes) = {
        let data = serialize_poa_setup(&poa_setup);
        let lock_script = ckb_packed::Script::new_builder()
            .code_hash(CKBPack::pack(&deployment_result.poa_state.script_type_hash))
            .hash_type(ScriptHashType::Type.into())
            .args(CKBPack::pack(
                &rollup_output.lock().calc_script_hash().as_bytes(),
            ))
            .build();
        let type_script = ckb_packed::Script::new_builder()
            .code_hash(CKBPack::pack(&TYPE_ID_CODE_HASH))
            .hash_type(ScriptHashType::Type.into())
            .args(CKBPack::pack(&poa_setup_cell_type_id))
            .build();
        let output = ckb_packed::CellOutput::new_builder()
            .lock(lock_script)
            .type_(CKBPack::pack(&Some(type_script)))
            .build();
        let output = fit_output_capacity(output, data.len());
        (output, data)
    };
    // 4. build PoA data cell (with type id)
    let (poa_data_output, poa_data_data): (ckb_packed::CellOutput, Bytes) = {
        let median_time = rpc_client.get_blockchain_info()?.median_time.0 / 1000;
        let poa_data = PoAData {
            round_initial_subtime: median_time,
            subblock_subtime: median_time,
            subblock_index: 0,
            aggregator_index: 0,
        };
        let data = serialize_poa_data(&poa_data);
        let lock_script = ckb_packed::Script::new_builder()
            .code_hash(CKBPack::pack(&deployment_result.poa_state.script_type_hash))
            .hash_type(ScriptHashType::Type.into())
            .args(CKBPack::pack(
                &rollup_output.lock().calc_script_hash().as_bytes(),
            ))
            .build();
        let type_script = ckb_packed::Script::new_builder()
            .code_hash(CKBPack::pack(&TYPE_ID_CODE_HASH))
            .hash_type(ScriptHashType::Type.into())
            .args(CKBPack::pack(&poa_data_cell_type_id))
            .build();
        let output = ckb_packed::CellOutput::new_builder()
            .lock(lock_script)
            .type_(CKBPack::pack(&Some(type_script)))
            .build();
        let output = fit_output_capacity(output, data.len());
        (output, data)
    };

    // 5. put genesis block in rollup'cell witness
    let witness_0: ckb_packed::WitnessArgs = {
        let output_type = genesis_with_global_state.genesis.as_bytes();
        ckb_packed::WitnessArgs::new_builder()
            .output_type(CKBPack::pack(&Some(output_type)))
            .build()
    };

    // 6. deploy genesis rollup cell
    let outputs_data = vec![rollup_data, poa_setup_data, poa_data_data];
    let outputs = vec![rollup_output, poa_setup_output, poa_data_output];
    let tx_hash = deploy_context.deploy(
        &mut rpc_client,
        outputs,
        outputs_data,
        vec![rollup_config_cell_dep.clone()],
        Some(&first_cell_input),
        witness_0,
    )?;

    // 7. write genesis deployment result
    let genesis_deployment_result = GenesisDeploymentResult {
        tx_hash,
        timestamp,
        rollup_type_hash: rollup_script_hash,
        rollup_type_script: rollup_type_script.into(),
        rollup_config: rollup_config.into(),
        rollup_config_cell_dep: rollup_config_cell_dep.into(),
        genesis_config,
        layer2_genesis_hash: genesis_with_global_state.genesis.hash().into(),
    };
    let output_content = serde_json::to_string_pretty(&genesis_deployment_result)
        .expect("serde json to string pretty");
    fs::write(output_path, output_content.as_bytes()).map_err(|err| err.to_string())?;
    Ok(())
}

fn calculate_type_id(first_cell_input: &ckb_packed::CellInput, first_output_index: u64) -> Bytes {
    let mut blake2b = new_blake2b();
    blake2b.update(first_cell_input.as_slice());
    blake2b.update(&first_output_index.to_le_bytes());
    let mut ret = [0; 32];
    blake2b.finalize(&mut ret);
    Bytes::from(ret.to_vec())
}

fn fit_output_capacity(output: ckb_packed::CellOutput, data_size: usize) -> ckb_packed::CellOutput {
    let data_capacity = Capacity::bytes(data_size).expect("data capacity");
    let capacity = output
        .occupied_capacity(data_capacity)
        .expect("occupied_capacity");
    output
        .as_builder()
        .capacity(CKBPack::pack(&capacity.as_u64()))
        .build()
}

fn collect_live_cells(
    rpc_client_url: &str,
    owner_address_str: &str,
    max_mature_number: u64,
    tip_number: u64,
    total_capacity: u64,
) -> Result<(Vec<ckb_packed::CellInput>, u64), String> {
    let number_step = 10000;
    let limit = Some(usize::max_value());
    let mut from_number = 0;
    let mut to_number = from_number + number_step - 1;
    let mut total_input_capacity = 0;
    let mut inputs = Vec::new();
    while total_input_capacity < total_capacity {
        if from_number > tip_number {
            return Err(format!(
                "not enough capacity from {}, expected: {}, found: {}",
                owner_address_str,
                HumanCapacity(total_capacity),
                HumanCapacity(total_input_capacity),
            ));
        }
        let new_cells = get_live_cells(
            rpc_client_url,
            owner_address_str,
            max_mature_number,
            Some(from_number),
            Some(to_number),
            limit,
        )?;
        for (new_input, new_capacity) in new_cells {
            total_input_capacity += new_capacity;
            inputs.push(new_input);
            if total_input_capacity >= total_capacity {
                break;
            }
        }
        from_number += number_step;
        to_number += number_step;
    }
    Ok((inputs, total_input_capacity))
}

// NOTE: This is an inefficient way to collect cells
fn get_live_cells(
    rpc_client_url: &str,
    owner_address_str: &str,
    max_mature_number: u64,
    from_number: Option<u64>,
    to_number: Option<u64>,
    limit: Option<usize>,
) -> Result<Vec<(ckb_packed::CellInput, u64)>, String> {
    let from_number_string = from_number.map(|value| value.to_string());
    let to_number_string = to_number.map(|value| value.to_string());
    let mut actual_limit = limit.unwrap_or(20);
    let mut cells = Vec::new();
    while cells.is_empty() {
        let limit_string = actual_limit.to_string();
        // wallet get-live-cells --address {address} --fast-mode --limit {limit} --from {from-number} --to {to-number}
        let mut args: Vec<&str> = vec![
            "--output-format",
            "json",
            "--url",
            rpc_client_url,
            "wallet",
            "get-live-cells",
            "--address",
            owner_address_str,
            "--fast-mode",
        ];
        if let Some(from_number) = from_number_string.as_ref() {
            args.push("--from");
            args.push(from_number.as_str());
        };
        if let Some(to_number) = to_number_string.as_ref() {
            args.push("--to");
            args.push(to_number.as_str());
        };
        args.push("--limit");
        args.push(limit_string.as_str());

        let live_cells_output = run_cmd(args)?;
        let live_cells: serde_json::Value =
            serde_json::from_str(&live_cells_output).map_err(|err| err.to_string())?;
        cells = live_cells["live_cells"]
            .as_array()
            .expect("josn live cells")
            .iter()
            .filter_map(|live_cell| {
                /*
                    {
                    "capacity": "1200.0 (CKB)",
                    "data_bytes": 968,
                    "index": {
                    "output_index": 0,
                    "tx_index": 1
                },
                    "lock_hash": "0x1cdeae55a5768fe14b628001c6247ae84c70310a7ddcfdc73ac68494251e46ec",
                    "mature": true,
                    "number": 6617,
                    "output_index": 0,
                    "tx_hash": "0x0d0d63184973ccdaf2c972783e1ed5f984a3e31b971e3294b092e54fe1d86961",
                    "type_hashes": null
                }
                     */
                let tx_index = live_cell["index"]["tx_index"]
                    .as_u64()
                    .expect("live cell tx_index");
                let number = live_cell["number"].as_u64().expect("live cell number");
                let data_bytes = live_cell["data_bytes"]
                    .as_u64()
                    .expect("live cell data_bytes");
                let type_is_null = live_cell["type_hashes"].is_null();
                if !type_is_null
                    || data_bytes > 0
                    || !is_mature(number, tx_index, max_mature_number)
                {
                    log::debug!(
                        "has type: {}, data not empty: {}, immature: {}, number: {}, tx_index: {}",
                        !type_is_null,
                        data_bytes > 0,
                        !is_mature(number, tx_index, max_mature_number),
                        number,
                        tx_index,
                    );
                    return None;
                }

                let input_tx_hash =
                    H256::from_str(&live_cell["tx_hash"].as_str().expect("live cell tx hash")[2..])
                        .expect("convert to h256");
                let input_index = live_cell["output_index"]
                    .as_u64()
                    .expect("live cell output index") as u32;
                let capacity = HumanCapacity::from_str(
                    live_cell["capacity"]
                        .as_str()
                        .expect("live cell capacity")
                        .split(' ')
                        .next()
                        .expect("capacity"),
                )
                .map(|human_capacity| human_capacity.0)
                .expect("parse capacity");
                let out_point =
                    ckb_packed::OutPoint::new(CKBPack::pack(&input_tx_hash), input_index);
                let input = ckb_packed::CellInput::new(out_point, 0);
                Some((input, capacity))
            })
            .collect();
        if actual_limit > u32::max_value() as usize / 2 {
            log::debug!("Can not find live cells for {}", owner_address_str);
            break;
        }
        actual_limit *= 2;
    }
    Ok(cells)
}

// Get max mature block number
pub fn get_max_mature_number(rpc_client: &mut HttpRpcClient) -> Result<u64, String> {
    let tip_epoch = rpc_client
        .get_tip_header()
        .map(|header| EpochNumberWithFraction::from_full_value(header.inner.epoch.0))?;
    let tip_epoch_number = tip_epoch.number();
    if tip_epoch_number < 4 {
        // No cellbase live cell is mature
        Ok(0)
    } else {
        let max_mature_epoch = rpc_client
            .get_epoch_by_number(tip_epoch_number - 4)?
            .ok_or_else(|| "Can not get epoch less than current epoch number".to_string())?;
        let start_number = max_mature_epoch.start_number;
        let length = max_mature_epoch.length;
        Ok(calc_max_mature_number(
            tip_epoch,
            Some((start_number, length)),
            CELLBASE_MATURITY,
        ))
    }
}

pub fn is_mature(number: u64, tx_index: u64, max_mature_number: u64) -> bool {
    // Not cellbase cell
    tx_index > 0
    // Live cells in genesis are all mature
        || number == 0
        || number <= max_mature_number
}

pub fn get_secp_data(
    rpc_client: &mut HttpRpcClient,
) -> Result<(Bytes, gw_jsonrpc_types::blockchain::CellDep), String> {
    let mut cell_dep = None;
    rpc_client
        .get_block_by_number(0)?
        .expect("get CKB genesis block")
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
    cell_dep.ok_or_else(|| "Can not found secp data cell in CKB genesis block".to_string())
}
