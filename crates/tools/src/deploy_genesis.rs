use std::iter::FromIterator;
use std::ops::{Deref, Sub};
use std::path::Path;
use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Context, Result};
use ckb_fixed_hash::H256;
use ckb_hash::new_blake2b;
use ckb_jsonrpc_types as rpc_types;
use ckb_resource::CODE_HASH_SECP256K1_DATA;
use ckb_sdk::{
    constants::{MIN_SECP_CELL_CAPACITY, ONE_CKB},
    traits::{CellCollector, CellQueryOptions, DefaultCellCollector, DefaultCellDepResolver},
    Address, AddressPayload, CkbRpcClient, SECP256K1,
};
use ckb_types::{
    bytes::{Bytes, BytesMut},
    core::{BlockView, Capacity, DepType, ScriptHashType, TransactionBuilder, TransactionView},
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
use tempfile::NamedTempFile;

use crate::types::{
    PoAConfig, PoASetup, RollupDeploymentResult, ScriptsDeploymentResult, UserRollupConfig,
};
use crate::utils::transaction::{get_network_type, run_cmd, wait_for_tx, TYPE_ID_CODE_HASH};

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

struct DeployContext<'a> {
    privkey_path: &'a Path,
    owner_address: &'a Address,
    cell_dep_resolver: DefaultCellDepResolver,
    deployment_result: &'a ScriptsDeploymentResult,
}

impl<'a> DeployContext<'a> {
    fn deploy(
        &mut self,
        rpc_client: &CkbRpcClient,
        mut outputs: Vec<ckb_packed::CellOutput>,
        mut outputs_data: Vec<Bytes>,
        mut deps: Vec<ckb_packed::CellDep>,
        first_cell_input: Option<&ckb_packed::CellInput>,
        witness_0: ckb_packed::WitnessArgs,
    ) -> Result<H256> {
        let tx_fee = ONE_CKB;
        let total_output_capacity: u64 = outputs
            .iter()
            .map(|output| {
                let value: u64 = CKBUnpack::unpack(&output.capacity());
                value
            })
            .sum();
        let total_capacity = total_output_capacity + tx_fee;

        let (inputs, total_input_capacity) = {
            let mut collector = DefaultCellCollector::new(rpc_client.url.as_str());
            let mut query = CellQueryOptions::new_lock(self.owner_address.payload().into());
            query.min_total_capacity = total_capacity;
            collector.collect_live_cells(&query, false)?
        };

        if let Some(first_input) = first_cell_input {
            if inputs[0].out_point.as_slice() != first_input.previous_output().as_slice() {
                return Err(anyhow!("first input cell changed"));
            }
        }
        let inputs = Vec::from_iter(
            inputs
                .into_iter()
                .map(|i| ckb_packed::CellInput::new(i.out_point, 0)),
        );

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
        let outputs_data: Vec<ckb_packed::Bytes> = outputs_data.iter().map(CKBPack::pack).collect();
        let sighash_dep = self.cell_dep_resolver.sighash_dep().unwrap().0.clone();
        deps.extend_from_slice(&[
            self.deployment_result
                .state_validator
                .cell_dep
                .clone()
                .into(),
            sighash_dep,
        ]);
        let tx: TransactionView = TransactionBuilder::default()
            .cell_deps(deps)
            .set_inputs(inputs)
            .set_outputs(outputs)
            .set_outputs_data(outputs_data)
            .set_witnesses(vec![CKBPack::pack(&witness_0.as_bytes())])
            .build();

        // 7. build ckb-cli tx and sign
        let tx_file = NamedTempFile::new()?;
        let tx_path_str = tx_file.path().to_str().unwrap();
        let _output = run_cmd([
            "--url",
            rpc_client.url.as_str(),
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
        std::fs::write(tx_path_str, cli_tx_content.as_bytes())?;
        let _output = run_cmd([
            "--url",
            rpc_client.url.as_str(),
            "tx",
            "sign-inputs",
            "--privkey-path",
            self.privkey_path.to_str().expect("non-utf8 file path"),
            "--tx-file",
            tx_path_str,
            "--add-signatures",
        ])?;

        // 8. send and then wait for tx
        let send_output = run_cmd([
            "--url",
            rpc_client.url.as_str(),
            "tx",
            "send",
            "--tx-file",
            tx_path_str,
            "--max-tx-fee",
            max_tx_fee_str,
            "--skip-check",
        ])?;
        let tx_hash = H256::from_str(send_output.trim().trim_start_matches("0x"))?;
        log::info!("tx_hash: {:#x}", tx_hash);
        wait_for_tx(rpc_client, &tx_hash, 120)?;
        Ok(tx_hash)
    }
}

pub struct DeployRollupCellArgs<'a> {
    pub privkey_path: &'a Path,
    pub ckb_rpc_url: &'a str,
    pub scripts_result: &'a ScriptsDeploymentResult,
    pub user_rollup_config: &'a UserRollupConfig,
    pub poa_config: &'a PoAConfig,
    pub timestamp: Option<u64>,
    pub skip_config_check: bool,
}

pub fn deploy_rollup_cell(args: DeployRollupCellArgs) -> Result<RollupDeploymentResult> {
    let DeployRollupCellArgs {
        privkey_path,
        ckb_rpc_url,
        scripts_result,
        user_rollup_config,
        poa_config,
        timestamp,
        skip_config_check,
    } = args;

    let poa_setup = poa_config.poa_setup.clone();

    let burn_lock_hash: [u8; 32] = {
        let lock: ckb_types::packed::Script = user_rollup_config.burn_lock.clone().into();
        lock.calc_script_hash().unpack().0
    };
    // check config
    if !skip_config_check {
        let expected_burn_lock_script = ckb_packed::Script::new_builder()
            .code_hash(CKBPack::pack(&[0u8; 32]))
            .hash_type(ScriptHashType::Data.into())
            .build();
        let expected_burn_lock_hash: [u8; 32] =
            expected_burn_lock_script.calc_script_hash().unpack().0;
        if H256(expected_burn_lock_hash) != H256(burn_lock_hash) {
            return Err(anyhow!(
                "The burn lock hash: 0x{} is not default, we suggest to use default burn lock \
                0x{} (code_hash: 0x, hash_type: Data, args: empty)",
                hex::encode(burn_lock_hash),
                hex::encode(expected_burn_lock_hash)
            ));
        }
        if poa_setup.round_intervals == 0 {
            return Err(anyhow!("round intervals value must be greater than 0"));
        }
    }

    let rpc_client = CkbRpcClient::new(ckb_rpc_url);
    let network_type = get_network_type(&rpc_client)?;
    let privkey_string = std::fs::read_to_string(privkey_path)?
        .split_whitespace()
        .next()
        .map(ToOwned::to_owned)
        .ok_or_else(|| anyhow!("File is empty"))?;
    let privkey_data = H256::from_str(privkey_string.trim().trim_start_matches("0x"))?;
    let privkey = secp256k1::SecretKey::from_slice(privkey_data.as_bytes())
        .map_err(|err| anyhow!("Invalid secp256k1 secret key format, error: {}", err))?;
    let pubkey = secp256k1::PublicKey::from_secret_key(&SECP256K1, &privkey);
    let owner_address_payload = AddressPayload::from_pubkey(&pubkey);
    let owner_address = Address::new(network_type, owner_address_payload, true);
    let genesis_block: BlockView = rpc_client
        .get_block_by_number(0.into())
        .map_err(|err| anyhow!(err))?
        .expect("Can not get genesis block?")
        .into();
    let cell_dep_resolver = DefaultCellDepResolver::from_genesis(&genesis_block)?;

    // deploy rollup config cell
    let allowed_contract_type_hashes: Vec<gw_packed::Byte32> = vec![
        GwPack::pack(&scripts_result.meta_contract_validator.script_type_hash),
        GwPack::pack(&scripts_result.l2_sudt_validator.script_type_hash),
        GwPack::pack(&scripts_result.polyjuice_validator.script_type_hash),
    ];

    // EOA scripts
    let mut allowed_eoa_type_hashes: Vec<gw_packed::Byte32> = vec![
        GwPack::pack(&scripts_result.eth_account_lock.script_type_hash),
        GwPack::pack(&scripts_result.tron_account_lock.script_type_hash),
    ];
    allowed_eoa_type_hashes.extend(
        user_rollup_config
            .allowed_eoa_type_hashes
            .clone()
            .into_iter()
            .map(|hash| GwPack::pack(&hash)),
    );
    allowed_eoa_type_hashes.dedup();

    // composite rollup config
    let rollup_config = RollupConfig::new_builder()
        .l1_sudt_script_type_hash(GwPack::pack(&user_rollup_config.l1_sudt_script_type_hash))
        .custodian_script_type_hash(GwPack::pack(
            &scripts_result.custodian_lock.script_type_hash,
        ))
        .deposit_script_type_hash(GwPack::pack(&scripts_result.deposit_lock.script_type_hash))
        .withdrawal_script_type_hash(GwPack::pack(
            &scripts_result.withdrawal_lock.script_type_hash,
        ))
        .challenge_script_type_hash(GwPack::pack(
            &scripts_result.challenge_lock.script_type_hash,
        ))
        .stake_script_type_hash(GwPack::pack(&scripts_result.stake_lock.script_type_hash))
        .l2_sudt_validator_script_type_hash(GwPack::pack(
            &scripts_result.l2_sudt_validator.script_type_hash,
        ))
        .burn_lock_hash(GwPack::pack(&burn_lock_hash))
        .required_staking_capacity(GwPack::pack(&user_rollup_config.required_staking_capacity))
        .challenge_maturity_blocks(GwPack::pack(&user_rollup_config.challenge_maturity_blocks))
        .finality_blocks(GwPack::pack(&user_rollup_config.finality_blocks))
        .reward_burn_rate(user_rollup_config.reward_burn_rate.into())
        .allowed_eoa_type_hashes(GwPackVec::pack(allowed_eoa_type_hashes))
        .allowed_contract_type_hashes(GwPackVec::pack(allowed_contract_type_hashes))
        .build();
    let (secp_data, secp_data_dep) = get_secp_data(&rpc_client)?;
    let mut deploy_context = DeployContext {
        privkey_path,
        owner_address: &owner_address,
        cell_dep_resolver,
        deployment_result: scripts_result,
    };

    let (rollup_config_output, rollup_config_data): (ckb_packed::CellOutput, Bytes) = {
        let data = rollup_config.as_bytes();
        let output = ckb_packed::CellOutput::new_builder()
            .lock(user_rollup_config.cells_lock.clone().into())
            .build();
        let output = fit_output_capacity(output, data.len());
        (output, data)
    };
    let rollup_config_cell_dep = {
        let tx_hash = deploy_context.deploy(
            &rpc_client,
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
        // New created CKB dev chain's may out of sync with real world time,
        // So we using an earlier time to get around this issue.
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("timestamp")
            .sub(core::time::Duration::from_secs(3600))
            .as_millis() as u64
    });

    let first_cell_input = {
        let mut collector = DefaultCellCollector::new(ckb_rpc_url);
        let cell_query = CellQueryOptions::new_lock(owner_address.payload().into());
        let first_input = collector
            .collect_live_cells(&cell_query, false)?
            .0
            .into_iter()
            .next()
            .context("no live cell found")?;
        ckb_packed::CellInput::new(first_input.out_point, 0)
    };

    let rollup_cell_type_id: Bytes = calculate_type_id(&first_cell_input, 0);
    let poa_setup_cell_type_id: Bytes = calculate_type_id(&first_cell_input, 1);
    let poa_data_cell_type_id: Bytes = calculate_type_id(&first_cell_input, 2);
    // calculate by: blake2b_hash(firstInput + rullupCell.outputIndex)
    let rollup_type_script = ckb_packed::Script::new_builder()
        .code_hash(CKBPack::pack(
            &scripts_result.state_validator.script_type_hash,
        ))
        .hash_type(ScriptHashType::Type.into())
        .args(CKBPack::pack(&rollup_cell_type_id))
        .build();
    let rollup_script_hash: H256 = CKBUnpack::unpack(&rollup_type_script.calc_script_hash());
    log::info!("rollup_script_hash: {:#x}", rollup_script_hash);

    // 1. build genesis block
    let genesis_config = GenesisConfig {
        timestamp,
        meta_contract_validator_type_hash: scripts_result
            .meta_contract_validator
            .script_type_hash
            .clone(),
        rollup_type_hash: rollup_script_hash.clone(),
        rollup_config: rollup_config.clone().into(),
        secp_data_dep,
    };
    let genesis_with_global_state = build_genesis(&genesis_config, secp_data)?;

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
                &scripts_result.state_validator_lock.script_type_hash,
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
            .code_hash(CKBPack::pack(&scripts_result.poa_state.script_type_hash))
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
        let median_time = u64::from(rpc_client.get_blockchain_info()?.median_time) / 1000;
        let poa_data = PoAData {
            round_initial_subtime: median_time,
            subblock_subtime: median_time,
            subblock_index: 0,
            aggregator_index: 0,
        };
        let data = serialize_poa_data(&poa_data);
        let lock_script = ckb_packed::Script::new_builder()
            .code_hash(CKBPack::pack(&scripts_result.poa_state.script_type_hash))
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

    // 5. put genesis block in rollup cell witness
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
        &rpc_client,
        outputs,
        outputs_data,
        vec![rollup_config_cell_dep.clone()],
        Some(&first_cell_input),
        witness_0,
    )?;

    // 7. write genesis deployment result
    let rollup_result = RollupDeploymentResult {
        tx_hash,
        timestamp,
        rollup_type_hash: rollup_script_hash,
        rollup_type_script: rollup_type_script.into(),
        rollup_config: rollup_config.into(),
        rollup_config_cell_dep: rollup_config_cell_dep.into(),
        genesis_config,
        layer2_genesis_hash: genesis_with_global_state.genesis.hash().into(),
    };
    Ok(rollup_result)
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

pub fn get_secp_data(
    rpc_client: &CkbRpcClient,
) -> Result<(Bytes, gw_jsonrpc_types::blockchain::CellDep)> {
    let mut cell_dep = None;
    rpc_client
        .get_block_by_number(0.into())?
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
    cell_dep.ok_or_else(|| anyhow!("Can not found secp data cell in CKB genesis block"))
}
