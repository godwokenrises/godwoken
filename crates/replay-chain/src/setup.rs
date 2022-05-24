use std::{path::PathBuf, sync::Arc};

use anyhow::{anyhow, Context as AnyHowContext, Result};
use ckb_types::{bytes::Bytes, prelude::Entity};
use gw_chain::chain::Chain;
use gw_config::{Config, StoreConfig};
use gw_db::{schema::COLUMNS, RocksDB};
use gw_generator::{
    account_lock_manage::{
        secp256k1::{Secp256k1Eth, Secp256k1Tron},
        AccountLockManage,
    },
    backend_manage::BackendManage,
    genesis::init_genesis,
    Generator,
};
use gw_rpc_client::{
    ckb_client::CKBClient, indexer_client::CKBIndexerClient, rpc_client::RPCClient,
};
use gw_store::Store;
use gw_types::{
    core::AllowedEoaType, offchain::RollupContext, packed::RollupConfig, prelude::Unpack,
};

pub struct SetupArgs {
    pub from_db_store: PathBuf,
    pub from_db_columns: u32,
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
        options: config.store.options.clone(),
        options_file: config.store.options_file.clone(),
        cache_size: config.store.cache_size,
    };
    let local_store = Store::new(RocksDB::open(&store_config, COLUMNS));
    let rollup_type_script = {
        let script: gw_types::packed::Script = config.chain.rollup_type_script.clone().into();
        script
    };
    let rollup_config: RollupConfig = config.genesis.rollup_config.clone().into();
    let rollup_context = RollupContext {
        rollup_config: rollup_config.clone(),
        rollup_script_hash: {
            let rollup_script_hash: [u8; 32] = config.genesis.rollup_type_hash.clone().into();
            rollup_script_hash.into()
        },
    };
    let secp_data: Bytes = {
        let rpc_client = {
            let indexer_client = CKBIndexerClient::with_url(&config.rpc_client.indexer_url)?;
            let ckb_client = CKBClient::with_url(&config.rpc_client.ckb_url)?;
            let rollup_type_script =
                ckb_types::packed::Script::new_unchecked(rollup_type_script.as_bytes());
            RPCClient::new(
                rollup_type_script,
                rollup_context.clone(),
                ckb_client,
                indexer_client,
            )
        };
        let out_point = config.genesis.secp_data_dep.out_point.clone();
        rpc_client
            .ckb
            .get_transaction(out_point.tx_hash.0.into())
            .await?
            .ok_or_else(|| anyhow!("can not found transaction: {:?}", out_point.tx_hash))?
            .raw()
            .outputs_data()
            .get(out_point.index.value() as usize)
            .expect("get secp output data")
            .raw_data()
    };

    init_genesis(
        &local_store,
        &config.genesis,
        config.chain.genesis_committed_info.clone().into(),
        secp_data,
    )
    .with_context(|| "init genesis")?;
    let generator = {
        let backend_manage = BackendManage::from_config(config.backends.clone())
            .with_context(|| "config backends")?;
        let mut account_lock_manage = AccountLockManage::default();
        let allowed_eoa_type_hashes = rollup_config.as_reader().allowed_eoa_type_hashes();
        let eth_lock_script_type_hash = allowed_eoa_type_hashes
            .iter()
            .find(|th| th.type_().to_entity() == AllowedEoaType::Eth.into())
            .ok_or_else(|| anyhow!("Eth: No allowed EoA type hashes in the rollup config"))?;
        account_lock_manage.register_lock_algorithm(
            eth_lock_script_type_hash.hash().unpack(),
            Box::new(Secp256k1Eth::default()),
        );
        let tron_lock_script_type_hash = allowed_eoa_type_hashes
            .iter()
            .find(|th| th.type_().to_entity() == AllowedEoaType::Tron.into());
        if let Some(type_hash) = tron_lock_script_type_hash {
            account_lock_manage.register_lock_algorithm(
                type_hash.hash().unpack(),
                Box::new(Secp256k1Tron::default()),
            )
        }
        Arc::new(Generator::new(
            backend_manage,
            account_lock_manage,
            rollup_context,
        ))
    };

    let chain = Chain::create(
        &rollup_config,
        &rollup_type_script,
        &config.chain,
        local_store.clone(),
        generator,
        None,
    )?;

    let from_store = {
        let store_config = StoreConfig {
            path: from_db_store,
            options: config.store.options.clone(),
            options_file: config.store.options_file.clone(),
            cache_size: config.store.cache_size,
        };
        Store::new(RocksDB::open(&store_config, from_db_columns))
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
