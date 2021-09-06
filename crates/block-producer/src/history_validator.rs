use anyhow::{anyhow, bail, Context, Result};
use async_jsonrpc_client::HttpClient;
use gw_challenge::{
    cancel_challenge::LoadDataStrategy,
    context::build_verify_context,
    offchain::{
        dump_tx, mock_cancel_challenge_tx,
        verify_tx::{verify_tx, TxWithContext},
        OffChainMockContext,
    },
};
use gw_common::{blake2b::new_blake2b, H256};
use gw_config::{Config, DebugConfig};
use gw_db::{schema::COLUMNS, RocksDB};
use gw_generator::{
    account_lock_manage::{
        secp256k1::{Secp256k1Eth, Secp256k1Tron},
        AccountLockManage,
    },
    backend_manage::BackendManage,
    Generator,
};
use gw_poa::PoA;
use gw_rpc_client::RPCClient;
use gw_store::Store;
use gw_types::{
    bytes::Bytes,
    core::{ChallengeTargetType, Status},
    offchain::RollupContext,
    packed::{ChallengeTarget, GlobalState, RollupConfig, Script},
    prelude::{Builder, Entity, Pack, Unpack},
};

use std::{
    collections::HashMap,
    fs::{create_dir_all, write},
    path::PathBuf,
    sync::Arc,
};

use crate::{utils::CKBGenesisInfo, wallet::Wallet};

const MAX_CYCLES: u64 = 7000_0000;

pub fn verify(config: Config, from_block: Option<u64>, to_block: Option<u64>) -> Result<()> {
    if config.store.path.as_os_str().is_empty() {
        bail!("empty store path, no history to verify");
    }
    if config.block_producer.is_none() {
        bail!("history validator require block producer config");
    }

    let validator = build_validator(config)?;
    validator.verify_history(from_block, to_block)?;

    Ok(())
}

fn build_validator(config: Config) -> Result<HistoryCancelChallengeValidator> {
    let rollup_config: RollupConfig = config.genesis.rollup_config.clone().into();
    let rollup_context = RollupContext {
        rollup_config: rollup_config.clone(),
        rollup_script_hash: {
            let rollup_script_hash: [u8; 32] = config.genesis.rollup_type_hash.clone().into();
            rollup_script_hash.into()
        },
    };
    let rollup_type_script: Script = config.chain.rollup_type_script.clone().into();

    let rpc_client = {
        let indexer_client = HttpClient::new(config.rpc_client.indexer_url)?;
        let ckb_client = HttpClient::new(config.rpc_client.ckb_url)?;
        let rollup_type_script =
            ckb_types::packed::Script::new_unchecked(rollup_type_script.as_bytes());
        RPCClient {
            indexer_client,
            ckb_client,
            rollup_context: rollup_context.clone(),
            rollup_type_script,
        }
    };

    let generator = {
        let backend_manage = BackendManage::from_config(config.backends.clone())
            .with_context(|| "config backends")?;
        let mut account_lock_manage = AccountLockManage::default();
        let eth_lock_script_type_hash = rollup_config
            .allowed_eoa_type_hashes()
            .get(0)
            .ok_or_else(|| anyhow!("Eth: No allowed EoA type hashes in the rollup config"))?;
        account_lock_manage.register_lock_algorithm(
            eth_lock_script_type_hash.unpack(),
            Box::new(Secp256k1Eth::default()),
        );
        let tron_lock_script_type_hash = rollup_config.allowed_eoa_type_hashes().get(1);
        if let Some(code_hash) = tron_lock_script_type_hash {
            account_lock_manage
                .register_lock_algorithm(code_hash.unpack(), Box::new(Secp256k1Tron::default()))
        }
        Arc::new(Generator::new(
            backend_manage,
            account_lock_manage,
            rollup_context.clone(),
        ))
    };

    let db_config = gw_db::config::Config {
        path: config.store.path,
        options: Default::default(),
        options_file: Default::default(),
    };
    let store = Store::new(RocksDB::open(&db_config, COLUMNS));

    let block_producer_config = config.block_producer.expect("should exists");
    let ckb_genesis_info = {
        let ckb_genesis = smol::block_on(async { rpc_client.get_block_by_number(0).await })?
            .ok_or_else(|| anyhow!("can't found CKB genesis block"))?;
        CKBGenesisInfo::from_block(&ckb_genesis)?
    };

    let secp_data: Bytes = {
        let out_point = config.genesis.secp_data_dep.out_point.clone();
        smol::block_on(rpc_client.get_transaction(out_point.tx_hash.0.into()))?
            .ok_or_else(|| anyhow!("can not found transaction: {:?}", out_point.tx_hash))?
            .raw()
            .outputs_data()
            .get(out_point.index.value() as usize)
            .expect("get secp output data")
            .raw_data()
    };

    let to_hash = |data| -> [u8; 32] {
        let mut hasher = new_blake2b();
        hasher.update(data);
        let mut hash = [0u8; 32];
        hasher.finalize(&mut hash);
        hash
    };
    let mut builtin_load_data = HashMap::new();
    builtin_load_data.insert(
        to_hash(secp_data.as_ref()).into(),
        config.genesis.secp_data_dep.clone().into(),
    );

    let wallet =
        Wallet::from_config(&block_producer_config.wallet_config).with_context(|| "init wallet")?;

    let poa = {
        let poa = PoA::new(
            rpc_client.clone(),
            wallet.lock_script().to_owned(),
            block_producer_config.poa_lock_dep.clone().into(),
            block_producer_config.poa_state_dep.clone().into(),
        );
        Arc::new(smol::lock::Mutex::new(poa))
    };

    let ckb_genesis_info = gw_challenge::offchain::CKBGenesisInfo {
        sighash_dep: ckb_genesis_info.sighash_dep(),
    };

    let offchain_mock_context = smol::block_on(async {
        let wallet = {
            let config = &block_producer_config.wallet_config;
            gw_challenge::Wallet::from_config(config).with_context(|| "init wallet")?
        };
        let poa = poa.lock().await;

        OffChainMockContext::build(
            &rpc_client,
            &poa,
            rollup_context.clone(),
            wallet,
            block_producer_config.clone(),
            ckb_genesis_info,
            builtin_load_data.clone(),
        )
        .await
    })?;

    let validator =
        HistoryCancelChallengeValidator::new(generator, store, offchain_mock_context, config.debug);

    Ok(validator)
}

struct HistoryCancelChallengeValidator {
    generator: Arc<Generator>,
    store: Store,
    mock_ctx: OffChainMockContext,
    debug_config: DebugConfig,
}

impl HistoryCancelChallengeValidator {
    fn new(
        generator: Arc<Generator>,
        store: Store,
        mock_ctx: OffChainMockContext,
        debug_config: DebugConfig,
    ) -> Self {
        HistoryCancelChallengeValidator {
            generator,
            store,
            mock_ctx,
            debug_config,
        }
    }

    fn verify_history(&self, from_block: Option<u64>, to_block: Option<u64>) -> Result<()> {
        let db = self.store.begin_transaction();
        let from_block = from_block.unwrap_or_else(|| 0);
        let to_block = match to_block {
            Some(to) => to,
            None => db.get_tip_block()?.raw().number().unpack(),
        };

        for block_number in from_block..=to_block {
            self.verify_block(block_number)?;
        }

        Ok(())
    }

    fn verify_block(&self, block_number: u64) -> Result<()> {
        let db = self.store.begin_transaction();
        log::info!("verify block #{}", block_number);

        let block_hash: H256 = {
            let maybe = db.get_block_hash_by_number(block_number)?;
            maybe.ok_or_else(|| anyhow!("block #{} not found", block_number))?
        };
        let global_state = {
            let maybe = db.get_block_post_global_state(&block_hash)?;
            let state =
                maybe.ok_or_else(|| anyhow!("block #{} global state not found", block_number))?;
            let to_builder = state.as_builder().status((Status::Halting as u8).into());
            to_builder.build()
        };
        let block = {
            let maybe = db.get_block(&block_hash)?;
            maybe.ok_or_else(|| anyhow!("block #{} not found", block_number))?
        };

        self.verify_withdrawals(global_state.clone(), block_hash, block.withdrawals().len())?;
        self.verify_txs(global_state.clone(), block_hash, block.transactions().len())?;

        Ok(())
    }

    fn verify_withdrawals(
        &self,
        global_state: GlobalState,
        block_hash: H256,
        count: usize,
    ) -> Result<()> {
        for idx in 0..(count as u32) {
            log::info!("verify withdrawal #{}", idx);
            let target = build_challenge_target(block_hash, idx, ChallengeTargetType::Withdrawal);
            self.verify(global_state.clone(), target)?;
        }

        Ok(())
    }

    fn verify_txs(&self, global_state: GlobalState, block_hash: H256, count: usize) -> Result<()> {
        for idx in 0..(count as u32) {
            log::info!("verify tx #{}", idx);
            let target = build_challenge_target(block_hash, idx, ChallengeTargetType::TxSignature);
            self.verify(global_state.clone(), target)?;

            let target = build_challenge_target(block_hash, idx, ChallengeTargetType::TxExecution);
            self.verify(global_state.clone(), target)?;
        }

        Ok(())
    }

    fn verify(&self, global_state: GlobalState, challenge_target: ChallengeTarget) -> Result<()> {
        let db = self.store.begin_transaction();
        let verify_context =
            build_verify_context(Arc::clone(&self.generator), &db, &challenge_target)?;

        let verify_with_strategy = |load_data_strategy: LoadDataStrategy| -> Result<()> {
            let mock_output = mock_cancel_challenge_tx(
                &self.mock_ctx.mock_rollup,
                &self.mock_ctx.mock_poa,
                global_state.clone(),
                challenge_target.clone(),
                verify_context.clone(),
                Some(load_data_strategy),
            )?;

            let result = verify_tx(
                &self.mock_ctx.rollup_cell_deps,
                TxWithContext::from(mock_output.clone()),
                MAX_CYCLES,
            );

            if result.is_err() {
                self.dump_tx_to_file(
                    load_data_strategy,
                    &challenge_target,
                    TxWithContext::from(mock_output),
                );
                result?;
            }

            Ok(())
        };

        if verify_with_strategy(LoadDataStrategy::Witness).is_err() {
            verify_with_strategy(LoadDataStrategy::CellDep)?;
        }

        Ok(())
    }

    fn dump_tx_to_file(
        &self,
        strategy: LoadDataStrategy,
        target: &ChallengeTarget,
        tx_with_context: TxWithContext,
    ) {
        let dump = || -> Result<_> {
            let debug_config = &self.debug_config;
            let dir = debug_config.debug_tx_dump_path.as_path();
            create_dir_all(&dir)?;

            let mut dump_path = PathBuf::new();
            dump_path.push(dir);

            let tx = dump_tx(&self.mock_ctx.rollup_cell_deps, tx_with_context)?;
            let dump_filename = format!("{:?}-{:?}-offchain-cancel-tx.json", target, strategy);
            dump_path.push(dump_filename);

            let json_tx = serde_json::to_string_pretty(&tx)?;
            log::info!("dump cancel tx from {:?} to {:?}", target, dump_path);
            write(dump_path, json_tx)?;

            Ok(())
        };

        if let Err(err) = dump() {
            log::error!("unable to dump offchain cancel challenge tx {}", err);
        }
    }
}

fn build_challenge_target(
    block_hash: H256,
    target_index: u32,
    target_type: ChallengeTargetType,
) -> ChallengeTarget {
    let target_type: u8 = target_type.into();
    ChallengeTarget::new_builder()
        .block_hash(block_hash.pack())
        .target_index(target_index.pack())
        .target_type(target_type.into())
        .build()
}
