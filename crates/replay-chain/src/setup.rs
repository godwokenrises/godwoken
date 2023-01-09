use std::{path::PathBuf, sync::Arc};

use anyhow::{anyhow, Context as AnyHowContext, Result};
use ckb_types::{bytes::Bytes, prelude::Entity};
use gw_chain::chain::Chain;
use gw_config::{Config, StoreConfig};
use gw_generator::{
    account_lock_manage::{secp256k1::Secp256k1Eth, AccountLockManage},
    backend_manage::BackendManage,
    genesis::init_genesis,
    Generator,
};
use gw_rpc_client::{
    ckb_client::CkbClient, indexer_client::CkbIndexerClient, rpc_client::RPCClient,
};
use gw_store::{schema::COLUMNS, Store};
use gw_types::{core::AllowedEoaType, packed::RollupConfig, prelude::Unpack};
use gw_utils::RollupContext;

pub struct SetupArgs {
    pub from_db_store: PathBuf,
    pub from_db_columns: usize,
    pub to_db_store: PathBuf,
    pub config: Config,
}

pub struct Context {
    pub chain: Chain,
    pub from_store: Store,
    pub local_store: Store,
}

pub async fn setup(args: SetupArgs) -> Result<Context> {
    let SetupArgs {
        from_db_store,
        to_db_store,
        config,
        from_db_columns,
    } = args;

    let store_config = StoreConfig {
        path: to_db_store,
        options_file: config.store.options_file.clone(),
        cache_size: config.store.cache_size,
    };
    let local_store = Store::open(&store_config, COLUMNS).unwrap();
    let rollup_type_script = {
        let script: gw_types::packed::Script = config.chain.rollup_type_script.clone().into();
        script
    };
    let rollup_config: RollupConfig = config.genesis.rollup_config.clone().into();
    let rollup_context = RollupContext {
        rollup_config: rollup_config.clone(),
        rollup_script_hash: config.genesis.rollup_type_hash.clone().into(),
        fork_config: config.fork.clone(),
    };
    let secp_data: Bytes = {
        let rpc_client = {
            let ckb_client = CkbClient::with_url(&config.rpc_client.ckb_url)?;
            let indexer_client = if let Some(ref indexer_url) = config.rpc_client.indexer_url {
                CkbIndexerClient::with_url(indexer_url)?
            } else {
                CkbIndexerClient::from(ckb_client.clone())
            };
            let rollup_type_script =
                ckb_types::packed::Script::new_unchecked(rollup_type_script.as_bytes());
            RPCClient::new(
                rollup_type_script,
                rollup_context.rollup_config.clone(),
                ckb_client,
                indexer_client,
            )
        };
        let out_point = config.genesis.secp_data_dep.out_point.clone();
        rpc_client
            .ckb
            .get_packed_transaction(out_point.tx_hash.0)
            .await?
            .ok_or_else(|| anyhow!("can not found transaction: {:?}", out_point.tx_hash))?
            .raw()
            .outputs_data()
            .get(out_point.index.value() as usize)
            .expect("get secp output data")
            .raw_data()
    };

    let genesis_tx_hash = config
        .chain
        .genesis_committed_info
        .transaction_hash
        .clone()
        .into();
    init_genesis(&local_store, &config.genesis, &genesis_tx_hash, secp_data)
        .with_context(|| "init genesis")?;
    let generator = {
        let backend_manage = BackendManage::from_config(config.fork.backend_forks.clone())
            .with_context(|| "config backends")?;
        let mut account_lock_manage = AccountLockManage::default();
        let allowed_eoa_type_hashes = rollup_config.as_reader().allowed_eoa_type_hashes();
        let eth_lock_script_type_hash = allowed_eoa_type_hashes
            .iter()
            .find(|th| th.type_().to_entity() == AllowedEoaType::Eth.into())
            .ok_or_else(|| anyhow!("Eth: No allowed EoA type hashes in the rollup config"))?;
        account_lock_manage.register_lock_algorithm(
            eth_lock_script_type_hash.hash().unpack(),
            Arc::new(Secp256k1Eth::default()),
        );
        Arc::new(Generator::new(
            backend_manage,
            account_lock_manage,
            rollup_context,
            Default::default(),
        ))
    };

    let chain = Chain::create(
        rollup_config,
        &rollup_type_script,
        &config.chain,
        local_store.clone(),
        generator,
        None,
    )?;

    let from_store = {
        let store_config = StoreConfig {
            path: from_db_store,
            options_file: config.store.options_file.clone(),
            cache_size: config.store.cache_size,
        };
        Store::open(&store_config, from_db_columns).unwrap()
    };

    println!(
        "Skip blocks: {:?}",
        config
            .chain
            .skipped_invalid_block_list
            .iter()
            .map(|hash| hash.to_string())
            .collect::<Vec<_>>()
    );

    Ok(Context {
        chain,
        from_store,
        local_store,
    })
}
