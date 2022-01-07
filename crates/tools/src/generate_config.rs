use crate::deploy_genesis::get_secp_data;
use crate::setup::get_wallet_info;
use crate::types::{
    BuildScriptsResult, RollupDeploymentResult, ScriptsDeploymentResult, UserRollupConfig,
};
use anyhow::{anyhow, Result};
use ckb_sdk::HttpRpcClient;
use ckb_types::prelude::{Builder, Entity};
use gw_config::{
    BackendConfig, BlockProducerConfig, ChainConfig, ChallengerConfig, Config, ConsensusConfig,
    ContractTypeScriptConfig, GenesisConfig, NodeMode, RPCClientConfig, RPCServerConfig,
    StoreConfig, WalletConfig, Web3IndexerConfig,
};
use gw_jsonrpc_types::godwoken::L2BlockCommittedInfo;
use gw_rpc_client::ckb_client::CKBClient;
use gw_types::{core::ScriptHashType, packed::Script, prelude::*};
use std::collections::HashMap;
use std::iter::FromIterator;
use std::path::Path;

pub struct GenerateNodeConfigArgs<'a> {
    pub rollup_result: &'a RollupDeploymentResult,
    pub scripts_deployment: &'a ScriptsDeploymentResult,
    pub privkey_path: &'a Path,
    pub ckb_url: String,
    pub indexer_url: String,
    pub database_url: Option<&'a str>,
    pub build_scripts_result: &'a BuildScriptsResult,
    pub server_url: String,
    pub user_rollup_config: &'a UserRollupConfig,
    pub node_mode: NodeMode,
}

pub fn generate_node_config(args: GenerateNodeConfigArgs) -> Result<Config> {
    let GenerateNodeConfigArgs {
        rollup_result,
        scripts_deployment,
        privkey_path,
        ckb_url,
        indexer_url,
        database_url,
        build_scripts_result,
        server_url,
        user_rollup_config,
        node_mode,
    } = args;

    let mut rpc_client = HttpRpcClient::new(ckb_url.to_string());
    let tx_with_status = rpc_client
        .get_transaction(rollup_result.tx_hash.clone())
        .map_err(|err| anyhow!("get transaction error: {}", err))?
        .ok_or_else(|| anyhow!("can't find genesis block transaction"))?;
    let block_hash = tx_with_status.tx_status.block_hash.ok_or_else(|| {
        anyhow!(
            "the genesis transaction haven't been packaged into chain, please retry after a while"
        )
    })?;
    let number = rpc_client
        .get_header(block_hash.clone())
        .map_err(|err| anyhow!("{}", err))?
        .ok_or_else(|| anyhow!("can't find block"))?
        .inner
        .number
        .into();

    // build configuration
    let account_id = 0;
    let node_wallet_info = get_wallet_info(privkey_path);
    let code_hash: [u8; 32] = {
        let mut hash = [0u8; 32];
        hex::decode_to_slice(
            node_wallet_info
                .block_assembler_code_hash
                .trim_start_matches("0x"),
            &mut hash as &mut [u8],
        )?;
        hash
    };
    let args = hex::decode(node_wallet_info.lock_arg.trim_start_matches("0x"))?;
    let lock = Script::new_builder()
        .code_hash(code_hash.pack())
        .hash_type(ScriptHashType::Type.into())
        .args(args.pack())
        .build()
        .into();

    let rollup_config = rollup_result.rollup_config.clone();
    let rollup_type_hash = rollup_result.rollup_type_hash.clone();
    let meta_contract_validator_type_hash = scripts_deployment
        .meta_contract_validator
        .script_type_hash
        .clone();
    let rollup_type_script = {
        let script: ckb_types::packed::Script = rollup_result.rollup_type_script.clone().into();
        gw_types::packed::Script::new_unchecked(script.as_bytes()).into()
    };
    let rollup_config_cell_dep = {
        let cell_dep: ckb_types::packed::CellDep =
            rollup_result.rollup_config_cell_dep.clone().into();
        gw_types::packed::CellDep::new_unchecked(cell_dep.as_bytes()).into()
    };
    let poa_lock_dep = {
        let dep: ckb_types::packed::CellDep = scripts_deployment
            .state_validator_lock
            .cell_dep
            .clone()
            .into();
        gw_types::packed::CellDep::new_unchecked(dep.as_bytes()).into()
    };
    let poa_state_dep = {
        let dep: ckb_types::packed::CellDep = scripts_deployment.poa_state.cell_dep.clone().into();
        gw_types::packed::CellDep::new_unchecked(dep.as_bytes()).into()
    };
    let (_data, secp_data_dep) =
        get_secp_data(&mut rpc_client).map_err(|err| anyhow!("get secp data {}", err))?;

    let ckb_client = CKBClient::with_url(&ckb_url)?;
    let contract_type_scripts = smol::block_on(query_contracts_script(
        &ckb_client,
        scripts_deployment,
        user_rollup_config,
    ))
    .map_err(|err| anyhow!("query contracts script {}", err))?;

    let challenger_config = ChallengerConfig {
        rewards_receiver_lock: {
            let lock: ckb_types::packed::Script = user_rollup_config.reward_lock.clone().into();
            let lock = gw_types::packed::Script::new_unchecked(lock.as_bytes());
            lock.into()
        },
        burn_lock: {
            let lock: ckb_types::packed::Script = user_rollup_config.burn_lock.clone().into();
            let lock = gw_types::packed::Script::new_unchecked(lock.as_bytes());
            lock.into()
        },
    };

    let wallet_config: WalletConfig = WalletConfig {
        privkey_path: privkey_path.into(),
        lock,
    };

    let backends: Vec<BackendConfig> = vec![
        BackendConfig {
            validator_path: build_scripts_result.built_scripts["meta_contract_validator"].clone(),
            generator_path: build_scripts_result.built_scripts["meta_contract_generator"].clone(),
            validator_script_type_hash: scripts_deployment
                .meta_contract_validator
                .script_type_hash
                .clone(),
            backend_type: gw_config::BackendType::Meta,
        },
        BackendConfig {
            validator_path: build_scripts_result.built_scripts["l2_sudt_validator"].clone(),
            generator_path: build_scripts_result.built_scripts["l2_sudt_generator"].clone(),
            validator_script_type_hash: scripts_deployment
                .l2_sudt_validator
                .script_type_hash
                .clone(),
            backend_type: gw_config::BackendType::Sudt,
        },
        BackendConfig {
            validator_path: build_scripts_result.built_scripts["polyjuice_validator"].clone(),
            generator_path: build_scripts_result.built_scripts["polyjuice_generator"].clone(),
            validator_script_type_hash: scripts_deployment
                .polyjuice_validator
                .script_type_hash
                .clone(),
            backend_type: gw_config::BackendType::Polyjuice,
        },
    ];

    let store = StoreConfig {
        path: "".into(),
        options: HashMap::new(),
        options_file: None,
        cache_size: None,
    };
    let genesis_committed_info = L2BlockCommittedInfo {
        block_hash,
        number,
        transaction_hash: rollup_result.tx_hash.clone(),
    };
    let chain: ChainConfig = ChainConfig {
        genesis_committed_info,
        rollup_type_script,
        skipped_invalid_block_list: Default::default(),
    };
    let rpc_client: RPCClientConfig = RPCClientConfig {
        indexer_url,
        ckb_url,
    };
    let rpc_server = RPCServerConfig {
        listen: server_url,
        ..Default::default()
    };
    let consensus = ConsensusConfig {
        contract_type_scripts,
    };
    let block_producer: Option<BlockProducerConfig> = Some(BlockProducerConfig {
        account_id,
        // cell deps
        rollup_config_cell_dep,
        poa_lock_dep,
        poa_state_dep,
        challenger_config,
        wallet_config,
        check_mem_block_before_submit: false,
        withdrawal_unlocker_wallet_config: None,
        ..Default::default()
    });
    let genesis: GenesisConfig = GenesisConfig {
        timestamp: rollup_result.timestamp,
        rollup_type_hash,
        meta_contract_validator_type_hash,
        rollup_config,
        secp_data_dep,
    };
    let eth_account_lock_hash = genesis
        .rollup_config
        .allowed_eoa_type_hashes
        .get(0)
        .ok_or_else(|| anyhow!("No allowed EoA type hashes in the rollup config"))?;
    let tron_allowed_eoa_hash = genesis.rollup_config.allowed_eoa_type_hashes.get(1);
    let tron_account_lock_hash = tron_allowed_eoa_hash.map(ToOwned::to_owned);

    let web3_indexer = database_url.map(|database_url| Web3IndexerConfig {
        database_url: database_url.to_owned(),
        polyjuice_script_type_hash: scripts_deployment
            .polyjuice_validator
            .script_type_hash
            .clone(),
        eth_account_lock_hash: eth_account_lock_hash.to_owned(),
        tron_account_lock_hash,
    });

    let config: Config = Config {
        backends,
        genesis,
        chain,
        rpc_client,
        rpc_server,
        rpc: Default::default(),
        consensus,
        block_producer,
        web3_indexer,
        node_mode,
        debug: Default::default(),
        offchain_validator: Default::default(),
        mem_pool: Default::default(),
        db_block_validator: Default::default(),
        store,
        fee: Default::default(),
        sentry_dsn: None,
    };

    Ok(config)
}

async fn query_contracts_script(
    ckb_client: &CKBClient,
    deployment: &ScriptsDeploymentResult,
    user_rollup_config: &UserRollupConfig,
) -> Result<ContractTypeScriptConfig> {
    use ckb_jsonrpc_types::CellDep;

    let query = |contract: &'static str, cell_dep: CellDep| -> _ {
        ckb_client.query_type_script(contract, cell_dep.into())
    };

    let state_validator = query(
        "state validator",
        deployment.state_validator.cell_dep.clone(),
    )
    .await?;
    assert_eq!(
        state_validator.hash(),
        deployment.state_validator.script_type_hash
    );

    let deposit_lock = query("deposit", deployment.deposit_lock.cell_dep.clone()).await?;
    assert_eq!(
        deposit_lock.hash(),
        deployment.deposit_lock.script_type_hash
    );

    let stake_lock = query("stake", deployment.stake_lock.cell_dep.clone()).await?;
    assert_eq!(stake_lock.hash(), deployment.stake_lock.script_type_hash);

    let custodian_lock = query("custodian", deployment.custodian_lock.cell_dep.clone()).await?;
    assert_eq!(
        custodian_lock.hash(),
        deployment.custodian_lock.script_type_hash
    );

    let withdrawal_lock = query("withdrawal", deployment.withdrawal_lock.cell_dep.clone()).await?;
    assert_eq!(
        withdrawal_lock.hash(),
        deployment.withdrawal_lock.script_type_hash
    );

    let challenge_lock = query("challenge", deployment.challenge_lock.cell_dep.clone()).await?;
    assert_eq!(
        challenge_lock.hash(),
        deployment.challenge_lock.script_type_hash
    );

    let l1_sudt = query("l1 sudt", user_rollup_config.l1_sudt_cell_dep.clone()).await?;
    assert_eq!(l1_sudt.hash(), user_rollup_config.l1_sudt_script_type_hash);

    // Allowed eoa script deps
    let eth_account_lock =
        query("eth account", deployment.eth_account_lock.cell_dep.clone()).await?;
    assert_eq!(
        eth_account_lock.hash(),
        deployment.eth_account_lock.script_type_hash
    );

    let tron_account_lock = query(
        "tron account",
        deployment.tron_account_lock.cell_dep.clone(),
    )
    .await?;
    assert_eq!(
        tron_account_lock.hash(),
        deployment.tron_account_lock.script_type_hash
    );
    // Allowed contract script deps
    let meta_validator = query("meta", deployment.meta_contract_validator.cell_dep.clone()).await?;
    assert_eq!(
        meta_validator.hash(),
        deployment.meta_contract_validator.script_type_hash
    );

    let l2_sudt_validator = query("l2 sudt", deployment.l2_sudt_validator.cell_dep.clone()).await?;
    assert_eq!(
        l2_sudt_validator.hash(),
        deployment.l2_sudt_validator.script_type_hash
    );

    let polyjuice_validator =
        query("polyjuice", deployment.polyjuice_validator.cell_dep.clone()).await?;
    assert_eq!(
        polyjuice_validator.hash(),
        deployment.polyjuice_validator.script_type_hash
    );

    let allowed_eoa_scripts: HashMap<_, _> = HashMap::from_iter([
        (eth_account_lock.hash(), eth_account_lock),
        (tron_account_lock.hash(), tron_account_lock),
    ]);

    let allowed_contract_scripts: HashMap<_, _> = HashMap::from_iter([
        (meta_validator.hash(), meta_validator),
        (l2_sudt_validator.hash(), l2_sudt_validator),
        (polyjuice_validator.hash(), polyjuice_validator),
    ]);

    Ok(ContractTypeScriptConfig {
        state_validator,
        deposit_lock,
        stake_lock,
        custodian_lock,
        withdrawal_lock,
        challenge_lock,
        l1_sudt,
        allowed_eoa_scripts,
        allowed_contract_scripts,
    })
}
