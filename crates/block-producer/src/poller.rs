#![allow(clippy::mutable_key_type)]

use crate::{
    types::ChainEvent,
    utils::{extract_deposit_requests, to_result},
};
use anyhow::{anyhow, Result};
use async_jsonrpc_client::{Params as ClientParams, Transport};
use ckb_fixed_hash::H256;
use gw_chain::chain::{
    Chain, ChallengeCell, L1Action, L1ActionContext, RevertL1ActionContext, RevertedAction,
    RevertedL1Action, RevertedLocalAction, SyncParam, UpdateAction,
};
use gw_jsonrpc_types::ckb_jsonrpc_types::{BlockNumber, HeaderView, TransactionWithStatus, Uint32};
use gw_rpc_client::{
    indexer_types::{Order, Pagination, ScriptType, SearchKey, SearchKeyFilter, Tx},
    rpc_client::RPCClient,
};
use gw_types::{
    bytes::Bytes,
    core::ScriptHashType,
    offchain::{global_state_from_slice, RollupContext, TxStatus},
    packed::{
        CellInput, ChallengeLockArgs, ChallengeLockArgsReader, L2BlockCommittedInfo, OutPoint,
        RollupAction, RollupActionUnion, Script, Transaction, WitnessArgs, WitnessArgsReader,
    },
    prelude::*,
};
use gw_web3_indexer::indexer::Web3Indexer;
use serde_json::json;
use smol::lock::Mutex;
use std::sync::Arc;

pub struct ChainUpdater {
    chain: Arc<Mutex<Chain>>,
    rpc_client: RPCClient,
    last_tx_hash: Option<H256>,
    rollup_context: RollupContext,
    rollup_type_script: ckb_types::packed::Script,
    web3_indexer: Option<Web3Indexer>,
    initialized: bool,
}

impl ChainUpdater {
    pub fn new(
        chain: Arc<Mutex<Chain>>,
        rpc_client: RPCClient,
        rollup_context: RollupContext,
        rollup_type_script: Script,
        web3_indexer: Option<Web3Indexer>,
    ) -> ChainUpdater {
        let rollup_type_script =
            ckb_types::packed::Script::new_unchecked(rollup_type_script.as_bytes());

        ChainUpdater {
            chain,
            rpc_client,
            rollup_context,
            rollup_type_script,
            last_tx_hash: None,
            web3_indexer,
            initialized: false,
        }
    }

    // Start syncing
    pub async fn handle_event(&mut self, _event: ChainEvent) -> Result<()> {
        let initial_syncing = !self.initialized;
        // Always start from last valid tip on l1
        if !self.initialized {
            self.revert_to_valid_tip_on_l1().await?;
            self.initialized = true;
        }

        // Check l1 fork
        let is_local_state_synced = {
            let chain = self.chain.lock().await;
            match chain.local_state().last_synced().to_owned() {
                Some(last_synced) => {
                    drop(chain);
                    self.find_l2block_on_l1(last_synced).await?
                }
                None => {
                    let pending_tx_hash = chain
                        .local_state()
                        .last_pending_commit()
                        .expect("pending commit")
                        .to_owned();
                    drop(chain);
                    self.find_l2block_in_pool(pending_tx_hash).await?
                }
            }
        };
        if !is_local_state_synced {
            self.revert_to_valid_tip_on_l1().await?;

            let (local_tip_block_number, local_tip_block_hash, local_tip_committed_info) = {
                let chain = self.chain.lock().await;
                let local_tip_block = chain.local_state().tip().raw();
                let local_tip_committed_info = chain.local_state().last_synced().to_owned();
                (
                    Unpack::<u64>::unpack(&local_tip_block.number()),
                    Into::<ckb_types::H256>::into(local_tip_block.hash()),
                    local_tip_committed_info,
                )
            };

            log::warn!(
                "revert to l2 block number {} hash {} on l1 {:?}",
                local_tip_block_number,
                local_tip_block_hash,
                local_tip_committed_info
            );
        }

        self.try_sync().await?;

        // Double check that we are synced to latest block
        // TODO: remove
        if let Some(rollup_cell) = self.rpc_client.query_rollup_cell().await? {
            let (local_tip_block_number, is_pending): (u64, bool) = {
                let chain = self.chain.lock().await;
                (
                    chain.local_state().tip().raw().number().unpack(),
                    chain.local_state().is_pending(),
                )
            };
            let tip_block_number = {
                let global_state = global_state_from_slice(&rollup_cell.data)?;
                Unpack::<u64>::unpack(&global_state.block().count()).saturating_sub(1)
            };

            if is_pending {
                assert!(
                    tip_block_number >= local_tip_block_number.saturating_sub(1),
                    "tip: {}, local tip: {}",
                    tip_block_number,
                    local_tip_block_number
                );

                // the new block is potentially a pending block
                if local_tip_block_number == tip_block_number {
                    log::info!("single update to latest l2 block {}", tip_block_number);

                    let tx_hash = rollup_cell.out_point.tx_hash().unpack();
                    self.update_single(&tx_hash).await?;
                }
            } else {
                assert!(
                    tip_block_number >= local_tip_block_number,
                    "tip: {}, local tip: {}",
                    tip_block_number,
                    local_tip_block_number
                );

                if local_tip_block_number.saturating_add(1) == tip_block_number {
                    log::info!("single update to latest l2 block {}", tip_block_number);

                    let tx_hash = rollup_cell.out_point.tx_hash().unpack();
                    self.update_single(&tx_hash).await?;
                }
            }
        }

        if initial_syncing {
            // Start notify mem pool after synced
            self.chain.lock().await.complete_initial_syncing()?;
        }

        Ok(())
    }

    pub async fn try_sync(&mut self) -> anyhow::Result<()> {
        let valid_tip_l1_block_number = {
            let chain = self.chain.lock().await;
            let local_tip_block: u64 = chain.local_state().tip().raw().number().unpack();
            let local_committed_l1_block: u64 = match chain.local_state().last_synced() {
                Some(last_synced) => last_synced.number().unpack(),
                None => {
                    // pending block, return it's parent's committed info
                    let db = chain.store().begin_transaction();
                    let committed_info = db
                        .get_l2block_committed_info(
                            &chain.local_state().tip().raw().parent_block_hash().unpack(),
                        )?
                        .expect("get parent committed info");
                    committed_info.number().unpack()
                }
            };

            log::debug!(
                "try sync from l2 block {} (l1 block {})",
                local_tip_block,
                local_committed_l1_block
            );

            local_committed_l1_block
        };
        let search_key = SearchKey {
            script: self.rollup_type_script.clone().into(),
            script_type: ScriptType::Type,
            filter: Some(SearchKeyFilter {
                script: None,
                output_data_len_range: None,
                output_capacity_range: None,
                block_range: Some([
                    BlockNumber::from(valid_tip_l1_block_number + 1),
                    BlockNumber::from(u64::max_value()),
                ]),
            }),
        };
        let order = Order::Asc;
        let limit = Uint32::from(1000);

        // TODO: the syncing logic here works under the assumption that a single
        // L1 CKB block can contain at most one L2 Godwoken block. The logic
        // here needs revising, once we relax this constraint for more performance.
        let mut last_cursor = None;
        loop {
            let txs: Pagination<Tx> = to_result(
                self.rpc_client
                    .indexer
                    .client()
                    .request(
                        "get_transactions",
                        Some(ClientParams::Array(vec![
                            json!(search_key),
                            json!(order),
                            json!(limit),
                            json!(last_cursor),
                        ])),
                    )
                    .await?,
            )?;
            if txs.objects.is_empty() {
                break;
            }
            last_cursor = Some(txs.last_cursor);

            log::debug!("Poll transactions: {}", txs.objects.len());
            self.update(&txs.objects).await?;
        }

        Ok(())
    }

    pub async fn update(&mut self, txs: &[Tx]) -> anyhow::Result<()> {
        for tx in txs.iter() {
            self.update_single(&tx.tx_hash).await?;
        }

        Ok(())
    }

    async fn update_single(&mut self, tx_hash: &H256) -> anyhow::Result<()> {
        if let Some(last_tx_hash) = &self.last_tx_hash {
            if last_tx_hash == tx_hash {
                log::info!("known last tx hash, skip {}", tx_hash);
                return Ok(());
            }
        }
        self.last_tx_hash = Some(tx_hash.clone());

        let tx: Option<TransactionWithStatus> = to_result(
            self.rpc_client
                .ckb
                .request(
                    "get_transaction",
                    Some(ClientParams::Array(vec![json!(tx_hash)])),
                )
                .await?,
        )?;
        let tx_with_status =
            tx.ok_or_else(|| anyhow::anyhow!("Cannot locate transaction: {:x}", tx_hash))?;
        let tx = {
            let tx: ckb_types::packed::Transaction = tx_with_status.transaction.inner.into();
            Transaction::new_unchecked(tx.as_bytes())
        };
        let block_hash = tx_with_status.tx_status.block_hash.ok_or_else(|| {
            anyhow::anyhow!("Transaction {:x} is not committed on chain!", tx_hash)
        })?;
        let header_view: Option<HeaderView> = to_result(
            self.rpc_client
                .ckb
                .request(
                    "get_header",
                    Some(ClientParams::Array(vec![json!(block_hash)])),
                )
                .await?,
        )?;
        let header_view =
            header_view.ok_or_else(|| anyhow::anyhow!("Cannot locate block: {:x}", block_hash))?;
        let l2block_committed_info = L2BlockCommittedInfo::new_builder()
            .number(header_view.inner.number.value().pack())
            .block_hash(block_hash.0.pack())
            .transaction_hash(tx_hash.pack())
            .build();

        let rollup_action = self.extract_rollup_action(&tx)?;
        let context = match rollup_action.to_enum() {
            RollupActionUnion::RollupSubmitBlock(submitted) => {
                let (requests, asset_type_scripts) =
                    extract_deposit_requests(&self.rpc_client, &self.rollup_context, &tx).await?;

                L1ActionContext::SubmitBlock {
                    l2block: submitted.block(),
                    deposit_requests: requests,
                    deposit_asset_scripts: asset_type_scripts,
                }
            }
            RollupActionUnion::RollupEnterChallenge(entered) => {
                let (challenge_cell, challenge_lock_args) =
                    self.extract_challenge_context(&tx).await?;

                L1ActionContext::Challenge {
                    cell: challenge_cell,
                    target: challenge_lock_args.target(),
                    witness: entered.witness(),
                }
            }
            RollupActionUnion::RollupCancelChallenge(_) => L1ActionContext::CancelChallenge,
            RollupActionUnion::RollupRevert(reverted) => {
                let reverted_blocks = reverted.reverted_blocks().into_iter();
                L1ActionContext::Revert {
                    reverted_blocks: reverted_blocks.collect(),
                }
            }
        };

        let update = L1Action {
            transaction: tx.clone(),
            l2block_committed_info,
            context,
        };
        self.chain
            .lock()
            .await
            .sync(SyncParam::Update(UpdateAction::L1(update)))?;

        // TODO sync missed block
        match &self.web3_indexer {
            Some(indexer) => {
                let store = { self.chain.lock().await.store().to_owned() };
                if let Err(err) = indexer.store(store, &tx).await {
                    log::error!("Web3 indexer store failed: {:?}", err);
                }
            }
            None => {}
        }

        Ok(())
    }

    async fn find_l2block_in_pool(&self, tx_hash: gw_common::H256) -> Result<bool> {
        let rpc_client = &self.rpc_client;
        let tx_status = rpc_client.get_transaction_status(tx_hash).await?;
        Ok(tx_status.is_some())
    }

    async fn find_l2block_on_l1(&self, committed_info: L2BlockCommittedInfo) -> Result<bool> {
        let rpc_client = &self.rpc_client;
        let tx_hash: gw_common::H256 =
            From::<[u8; 32]>::from(committed_info.transaction_hash().unpack());
        let tx_status = rpc_client.get_transaction_status(tx_hash).await?;
        if !matches!(tx_status, Some(TxStatus::Committed)) {
            return Ok(false);
        }

        let block_hash: [u8; 32] = committed_info.block_hash().unpack();
        let l1_block_hash = rpc_client.get_transaction_block_hash(tx_hash).await?;
        Ok(l1_block_hash == Some(block_hash))
    }

    async fn revert_to_valid_tip_on_l1(&mut self) -> Result<()> {
        let db = { self.chain.lock().await.store().begin_transaction() };
        let mut revert_l1_actions = Vec::new();

        // First rewind to last valid tip
        let last_valid_tip_block_hash = db.get_last_valid_tip_block_hash()?;
        let last_valid_tip_global_state = db
            .get_block_post_global_state(&last_valid_tip_block_hash)?
            .expect("valid tip global status should exists");
        let last_valid_tip_committed_info_opt =
            db.get_l2block_committed_info(&last_valid_tip_block_hash)?;
        let rewind_to_last_valid_tip = match last_valid_tip_committed_info_opt.clone() {
            Some(last_valid_tip_committed_info) => RevertedAction::L1(RevertedL1Action {
                prev_global_state: last_valid_tip_global_state,
                l2block_committed_info: last_valid_tip_committed_info,
                context: RevertL1ActionContext::RewindToLastValidTip,
            }),
            None => RevertedAction::Local(RevertedLocalAction {
                prev_global_state: last_valid_tip_global_state,
                context: RevertL1ActionContext::RewindToLastValidTip,
            }),
        };
        revert_l1_actions.push(rewind_to_last_valid_tip);

        // Revert until last valid block on l1 found
        let mut local_valid_committed_info_opt = last_valid_tip_committed_info_opt;
        let mut local_valid_block = db.get_last_valid_tip_block()?;
        loop {
            if local_valid_committed_info_opt.is_some()
                && self
                    .find_l2block_on_l1(local_valid_committed_info_opt.clone().unwrap())
                    .await?
            {
                break;
            }

            let parent_valid_block_hash: [u8; 32] =
                local_valid_block.raw().parent_block_hash().unpack();
            let parent_valid_global_state = db
                .get_block_post_global_state(&parent_valid_block_hash.into())?
                .expect("valid tip global status should exists");
            let parent_valid_committed_info = db
                .get_l2block_committed_info(&parent_valid_block_hash.into())?
                .expect("valid block l2 committed info should exists");

            let block_is_committed = local_valid_committed_info_opt.is_some();

            let revert_submit_valid_block = if block_is_committed {
                // revert committed block
                RevertedAction::L1(RevertedL1Action {
                    prev_global_state: parent_valid_global_state,
                    l2block_committed_info: parent_valid_committed_info.clone(),
                    context: RevertL1ActionContext::SubmitValidBlock {
                        l2block: local_valid_block,
                    },
                })
            } else {
                // revert pending block
                RevertedAction::Local(RevertedLocalAction {
                    prev_global_state: parent_valid_global_state,
                    context: RevertL1ActionContext::SubmitValidBlock {
                        l2block: local_valid_block,
                    },
                })
            };
            revert_l1_actions.push(revert_submit_valid_block);

            local_valid_committed_info_opt = Some(parent_valid_committed_info);
            local_valid_block = db
                .get_block(&parent_valid_block_hash.into())?
                .expect("valid block should exists");
        }

        for action in revert_l1_actions {
            self.chain.lock().await.sync(SyncParam::Revert(action))?;
        }

        // Also revert last tx hash
        let local_tip_committed_info = db
            .get_l2block_committed_info(&db.get_tip_block_hash()?)?
            .expect("tip committed info should exists");
        let local_tip_committed_l1_tx_hash = local_tip_committed_info.transaction_hash().unpack();
        self.last_tx_hash = Some(local_tip_committed_l1_tx_hash);

        Ok(())
    }

    fn extract_rollup_action(&self, tx: &Transaction) -> Result<RollupAction> {
        let rollup_type_hash: [u8; 32] = {
            let hash = self.rollup_type_script.calc_script_hash();
            ckb_types::prelude::Unpack::unpack(&hash)
        };

        // find rollup state cell from outputs
        let (i, _) = {
            let outputs = tx.raw().outputs().into_iter();
            let find_rollup = outputs.enumerate().find(|(_i, output)| {
                output.type_().to_opt().map(|type_| type_.hash()) == Some(rollup_type_hash)
            });
            find_rollup.ok_or_else(|| anyhow!("no rollup cell found"))?
        };

        let witness: Bytes = {
            let rollup_witness = tx.witnesses().get(i).ok_or_else(|| anyhow!("no witness"))?;
            rollup_witness.unpack()
        };

        let witness_args = match WitnessArgsReader::verify(&witness, false) {
            Ok(_) => WitnessArgs::new_unchecked(witness),
            Err(_) => return Err(anyhow!("invalid witness")),
        };

        let output_type: Bytes = {
            let type_ = witness_args.output_type();
            let should_exist = type_.to_opt().ok_or_else(|| anyhow!("no output type"))?;
            should_exist.unpack()
        };

        RollupAction::from_slice(&output_type).map_err(|e| anyhow!("invalid rollup action {}", e))
    }

    async fn extract_challenge_context(
        &self,
        tx: &Transaction,
    ) -> Result<(ChallengeCell, ChallengeLockArgs)> {
        let challenge_script_type_hash = self
            .rollup_context
            .rollup_config
            .challenge_script_type_hash();

        let outputs = tx.as_reader().raw().outputs();
        let outputs_data = tx.as_reader().raw().outputs_data();
        for (index, (output, output_data)) in outputs.iter().zip(outputs_data.iter()).enumerate() {
            if output.lock().code_hash().as_slice() != challenge_script_type_hash.as_slice()
                || output.lock().hash_type().to_entity() != ScriptHashType::Type.into()
            {
                continue;
            }

            let lock_args = {
                let args: Bytes = output.lock().args().unpack();
                match ChallengeLockArgsReader::verify(&args.slice(32..), false) {
                    Ok(_) => ChallengeLockArgs::new_unchecked(args.slice(32..)),
                    Err(err) => return Err(anyhow!("invalid challenge lock args {}", err)),
                }
            };

            let input = {
                let out_point = OutPoint::new_builder()
                    .tx_hash(tx.hash().pack())
                    .index((index as u32).pack())
                    .build();

                CellInput::new_builder().previous_output(out_point).build()
            };

            let cell = ChallengeCell {
                input,
                output: output.to_entity(),
                output_data: output_data.unpack(),
            };

            return Ok((cell, lock_args));
        }

        unreachable!("challenge output not found");
    }
}
