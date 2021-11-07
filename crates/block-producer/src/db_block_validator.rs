use anyhow::{anyhow, bail, Context, Result};
use gw_challenge::{
    cancel_challenge::LoadDataStrategy,
    context::build_verify_context,
    offchain::{
        dump_tx, mock_cancel_challenge_tx,
        verify_tx::{verify_tx, TxWithContext},
        OffChainMockContext,
    },
};
use gw_common::H256;
use gw_config::{Config, DBBlockValidatorConfig, DebugConfig};
use gw_generator::Generator;
use gw_jsonrpc_types::godwoken::ChallengeTargetType as JsonChallengeTargetType;
use gw_store::Store;
use gw_types::{
    core::{ChallengeTargetType, Status},
    packed::{ChallengeTarget, GlobalState, L2Block},
    prelude::{Builder, Entity, Pack, Unpack},
};
use gw_utils::wallet::Wallet;
use rayon::prelude::*;

use std::{
    fs::{create_dir_all, write},
    path::PathBuf,
    sync::Arc,
};

use crate::runner::BaseInitComponents;

pub fn verify(config: Config, from_block: Option<u64>, to_block: Option<u64>) -> Result<()> {
    if config.store.path.as_os_str().is_empty() {
        bail!("empty store path, no db block to verify");
    }
    if config.block_producer.is_none() {
        bail!("db block validator require block producer config");
    }

    let validator = build_validator(config)?;
    validator.verify_db(from_block, to_block)?;

    Ok(())
}

fn build_validator(config: Config) -> Result<DBBlockCancelChallengeValidator> {
    let base = BaseInitComponents::init(&config, true)?;
    let block_producer_config = config.block_producer.expect("block producer config");

    let wallet =
        Wallet::from_config(&block_producer_config.wallet_config).with_context(|| "init wallet")?;
    let poa = base.init_poa(&wallet, &block_producer_config);
    let mut offchain_mock_context = smol::block_on(async {
        let poa = poa.lock().await;
        base.init_offchain_mock_context(&poa, &block_producer_config)
            .await
    })?;

    let validator_config = config.db_block_validator.as_ref();
    if let Some(Some(scripts)) = validator_config.map(|c| c.replace_scripts.as_ref()) {
        offchain_mock_context = offchain_mock_context.replace_scripts(scripts)?;
    }
    let validator = DBBlockCancelChallengeValidator::new(
        base.generator,
        base.store,
        offchain_mock_context,
        config.debug,
        config.db_block_validator.unwrap_or_default(),
    );

    Ok(validator)
}

struct DBBlockCancelChallengeValidator {
    generator: Arc<Generator>,
    store: Store,
    mock_ctx: OffChainMockContext,
    debug_config: DebugConfig,
    config: DBBlockValidatorConfig,
}

impl DBBlockCancelChallengeValidator {
    fn new(
        generator: Arc<Generator>,
        store: Store,
        mock_ctx: OffChainMockContext,
        debug_config: DebugConfig,
        config: DBBlockValidatorConfig,
    ) -> Self {
        DBBlockCancelChallengeValidator {
            generator,
            store,
            mock_ctx,
            debug_config,
            config,
        }
    }

    fn verify_db(&self, from_block: Option<u64>, to_block: Option<u64>) -> Result<()> {
        let db = self.store.begin_transaction();
        let from_block = from_block.unwrap_or(0);
        let to_block = match to_block {
            Some(to) => to,
            None => db.get_tip_block()?.raw().number().unpack(),
        };

        if self.config.parallel_verify_blocks {
            (from_block..=to_block)
                .into_par_iter()
                .try_for_each(|block_number| self.verify_block(block_number))?;
        } else {
            (from_block..=to_block)
                .into_iter()
                .try_for_each(|block_number| self.verify_block(block_number))?;
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

        self.verify_withdrawals(global_state.clone(), &block)?;
        self.verify_txs(global_state, &block)?;

        Ok(())
    }

    fn verify_withdrawals(&self, global_state: GlobalState, block: &L2Block) -> Result<()> {
        let block_hash: H256 = block.hash().into();
        let block_number: u64 = block.raw().number().unpack();

        let verify_withdrawal = |idx| -> Result<()> {
            if self.config.parallel_verify_blocks {
                log::info!("verify block #{} withdrawal #{}", block_number, idx);
            } else {
                log::info!("verify withdrawal #{}", idx);
            }

            if let Some(ref skip_targets) = self.config.skip_targets {
                let key = (block_number, JsonChallengeTargetType::Withdrawal, idx);
                if skip_targets.contains(&key) {
                    log::info!(
                        "skip block #{} withdrawal #{} type: {:?}",
                        block_number,
                        idx,
                        ChallengeTargetType::Withdrawal,
                    );
                    return Ok(());
                }
            }

            let withdrawal = block.withdrawals().get(idx as usize).unwrap();
            let dump_context = DumpContext {
                block_number,
                target_type: ChallengeTargetType::Withdrawal,
                target_index: idx,
                target_hash: withdrawal.hash().into(),
            };

            let target = build_challenge_target(block_hash, idx, ChallengeTargetType::Withdrawal);
            self.verify(dump_context, global_state.clone(), target)?;

            Ok(())
        };

        (0..(block.withdrawals().len() as u32))
            .into_par_iter()
            .try_for_each(verify_withdrawal)?;

        Ok(())
    }

    fn verify_txs(&self, global_state: GlobalState, block: &L2Block) -> Result<()> {
        let block_hash: H256 = block.hash().into();
        let block_number: u64 = block.raw().number().unpack();

        let verify_tx =
            |idx: u32, target_hash: H256, target_type: ChallengeTargetType| -> Result<()> {
                if let Some(ref skip_targets) = self.config.skip_targets {
                    let key = (block_number, target_type.into(), idx);
                    if skip_targets.contains(&key) {
                        log::info!(
                            "skip block #{} tx #{} type: {:?}",
                            block_number,
                            idx,
                            target_type
                        );
                        return Ok(());
                    }
                }

                let dump_context = DumpContext {
                    block_number,
                    target_type,
                    target_index: idx,
                    target_hash,
                };

                let target = build_challenge_target(block_hash, idx, target_type);
                self.verify(dump_context, global_state.clone(), target)?;

                Ok(())
            };

        (0..(block.transactions().len() as u32))
            .into_par_iter()
            .try_for_each(|idx| {
                if self.config.parallel_verify_blocks {
                    log::info!("verify block #{} tx #{}", block_number, idx);
                } else {
                    log::info!("verify tx #{}", idx);
                }

                let tx = block.transactions().get(idx as usize).unwrap();
                let tx_hash = tx.hash().into();

                verify_tx(idx, tx_hash, ChallengeTargetType::TxSignature)?;
                verify_tx(idx, tx_hash, ChallengeTargetType::TxExecution)?;

                Ok::<_, anyhow::Error>(())
            })?;

        Ok(())
    }

    fn verify(
        &self,
        dump_context: DumpContext,
        global_state: GlobalState,
        challenge_target: ChallengeTarget,
    ) -> Result<()> {
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
                u64::MAX,
            );

            match result {
                Ok(used_cycles) if used_cycles > self.config.verify_max_cycles => {
                    self.dump_tx_to_file(
                        &dump_context,
                        load_data_strategy,
                        &format!("used-cycles-{}", used_cycles),
                        TxWithContext::from(mock_output),
                    );

                    Err(anyhow!(
                        "exceeded max cycles, used {} expect <= {}",
                        used_cycles,
                        self.config.verify_max_cycles
                    ))
                }
                Err(err) => {
                    self.dump_tx_to_file(
                        &dump_context,
                        load_data_strategy,
                        "",
                        TxWithContext::from(mock_output),
                    );
                    Err(err)
                }
                Ok(_) => Ok(()),
            }
        };

        if verify_with_strategy(LoadDataStrategy::Witness).is_err() {
            if let Err(err) = verify_with_strategy(LoadDataStrategy::CellDep) {
                if !err.to_string().contains("exceeded max cycles, used") {
                    return Err(err);
                }
            }
        }

        Ok(())
    }

    fn dump_tx_to_file(
        &self,
        dump_context: &DumpContext,
        load_data_strategy: LoadDataStrategy,
        addition_text: &str,
        tx_with_context: TxWithContext,
    ) {
        let dump = || -> Result<_> {
            let debug_config = &self.debug_config;
            let dir = debug_config.debug_tx_dump_path.as_path();
            create_dir_all(&dir)?;

            let mut dump_path = PathBuf::new();
            dump_path.push(dir);

            let tx = dump_tx(&self.mock_ctx.rollup_cell_deps, tx_with_context)?;
            let dump_info = dump_context.info_with_load_data_strategy(load_data_strategy);
            let dump_filename = format!("{}-{}-offchain-cancel-tx.json", dump_info, addition_text);
            dump_path.push(dump_filename);

            let json_tx = serde_json::to_string_pretty(&tx)?;
            log::info!("dump cancel tx from {:?} to {:?}", dump_info, dump_path);
            write(dump_path, json_tx)?;

            Ok(())
        };

        if let Err(err) = dump() {
            log::error!("unable to dump offchain cancel challenge tx {}", err);
        }
    }
}

#[derive(Clone)]
struct DumpContext {
    block_number: u64,
    target_type: ChallengeTargetType,
    target_index: u32,
    target_hash: H256,
}

impl DumpContext {
    fn info_with_load_data_strategy(&self, load_data_strategy: LoadDataStrategy) -> String {
        let type_ = match self.target_type {
            ChallengeTargetType::TxSignature => "tx-signature",
            ChallengeTargetType::TxExecution => "tx-execution",
            ChallengeTargetType::Withdrawal => "withdrawal",
        };
        let hash = ckb_types::H256(self.target_hash.into());
        let strategy = match load_data_strategy {
            LoadDataStrategy::Witness => "with-witness-load-data",
            LoadDataStrategy::CellDep => "with-celldep-load-data",
        };

        format!(
            "block-#{}-{}-{}-{}-{}",
            self.block_number, type_, self.target_index, hash, strategy
        )
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
