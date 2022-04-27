#![allow(clippy::mutable_key_type)]
#![allow(clippy::unnecessary_unwrap)]
//! MemPool
//!
//! The mem pool will update txs & withdrawals 'instantly' by running background tasks.
//! So a user could query the tx receipt 'instantly'.
//! Since we already got the next block status, the block prodcuer would not need to execute
//! txs & withdrawals again.
//!

use anyhow::{anyhow, Result};
use gw_common::{
    builtins::CKB_SUDT_ACCOUNT_ID,
    state::{to_short_address, State},
    H256,
};
use gw_config::{MemPoolConfig, NodeMode};
use gw_dynamic_config::manager::DynamicConfigManager;
use gw_generator::{
    constants::L2TX_MAX_CYCLES, error::TransactionError, generator::UnlockWithdrawal,
    traits::StateExt, ArcSwap, Generator,
};
use gw_rpc_ws_server::notify_controller::NotifyController;
use gw_store::{
    chain_view::ChainView,
    mem_pool_state::{MemPoolState, MemStore},
    traits::chain_store::ChainStore,
    transaction::StoreTransaction,
    Store,
};
use gw_traits::CodeStore;
use gw_types::{
    offchain::{CellStatus, DepositInfo, ErrorTxReceipt},
    packed::{
        AccountMerkleState, BlockInfo, L2Block, L2Transaction, Script, TxReceipt,
        WithdrawalRequest, WithdrawalRequestExtra,
    },
    prelude::{Entity, Pack, Unpack},
};
use std::{
    cmp::{max, min},
    collections::{HashMap, HashSet, VecDeque},
    iter::FromIterator,
    ops::Shr,
    sync::Arc,
    time::{Duration, Instant},
};
use tracing::instrument;

use crate::{
    constants::{MAX_MEM_BLOCK_TXS, MAX_MEM_BLOCK_WITHDRAWALS, MAX_TX_SIZE, MAX_WITHDRAWAL_SIZE},
    custodian::AvailableCustodians,
    mem_block::MemBlock,
    restore_manager::RestoreManager,
    sync::{mq::tokio_kafka, publish::MemPoolPublishService},
    traits::{MemPoolErrorTxHandler, MemPoolProvider},
    types::EntryList,
    withdrawal::Generator as WithdrawalGenerator,
};

#[derive(Debug)]
pub struct OutputParam {
    pub retry_count: usize,
}

impl OutputParam {
    pub fn new(retry_count: usize) -> Self {
        OutputParam { retry_count }
    }
}

impl Default for OutputParam {
    fn default() -> Self {
        OutputParam { retry_count: 0 }
    }
}

/// MemPool
pub struct MemPool {
    /// store
    store: Store,
    /// current tip
    current_tip: (H256, u64),
    /// generator instance
    generator: Arc<Generator>,
    /// error tx handler,
    error_tx_handler: Option<Box<dyn MemPoolErrorTxHandler + Send + Sync>>,
    /// error tx receipt notifier
    error_tx_receipt_notifier: Option<NotifyController>,
    /// pending queue, contains executable contents
    pending: HashMap<u32, EntryList>,
    /// memory block
    mem_block: MemBlock,
    /// Mem pool provider
    provider: Box<dyn MemPoolProvider + Send + Sync>,
    /// Pending deposits
    pending_deposits: Vec<DepositInfo>,
    /// Mem block save and restore
    restore_manager: RestoreManager,
    /// Restored txs to finalize
    pending_restored_tx_hashes: VecDeque<H256>,
    // Fan-out mem block from full node to readonly node
    mem_pool_publish_service: Option<MemPoolPublishService>,
    node_mode: NodeMode,
    mem_pool_state: Arc<MemPoolState>,
    dynamic_config_manager: Arc<ArcSwap<DynamicConfigManager>>,
}

pub struct MemPoolCreateArgs {
    pub block_producer_id: u32,
    pub store: Store,
    pub generator: Arc<Generator>,
    pub provider: Box<dyn MemPoolProvider + Send + Sync>,
    pub error_tx_handler: Option<Box<dyn MemPoolErrorTxHandler + Send + Sync>>,
    pub error_tx_receipt_notifier: Option<NotifyController>,
    pub config: MemPoolConfig,
    pub node_mode: NodeMode,
    pub dynamic_config_manager: Arc<ArcSwap<DynamicConfigManager>>,
}

impl Drop for MemPool {
    fn drop(&mut self) {
        log::info!("Saving mem block to {:?}", self.restore_manager().path());
        if let Err(err) = self.save_mem_block() {
            log::error!("Save mem block error {}", err);
        }
        self.restore_manager().delete_before_one_hour();
    }
}

impl MemPool {
    pub async fn create(args: MemPoolCreateArgs) -> Result<Self> {
        let MemPoolCreateArgs {
            block_producer_id,
            store,
            generator,
            provider,
            error_tx_handler,
            error_tx_receipt_notifier,
            config,
            node_mode,
            dynamic_config_manager,
        } = args;
        let pending = Default::default();

        let tip_block = {
            let db = store.begin_transaction();
            db.get_last_valid_tip_block()?
        };
        let tip = (tip_block.hash().into(), tip_block.raw().number().unpack());

        let mut mem_block = MemBlock::with_block_producer(block_producer_id);
        let mut pending_deposits = vec![];
        let mut pending_restored_tx_hashes = VecDeque::new();

        let restore_manager = RestoreManager::build(&config.restore_path)?;
        if let Ok(Some((restored, timestamp))) = restore_manager.restore_from_latest() {
            log::info!("[mem-pool] restore mem block from timestamp {}", timestamp);

            let hashes: Vec<_> = restored.withdrawals().unpack();
            mem_block.force_reinject_withdrawal_hashes(hashes.as_slice());

            pending_restored_tx_hashes = VecDeque::from(Unpack::<Vec<_>>::unpack(&restored.txs()));
            pending_deposits = restored.deposits().unpack();
        }

        mem_block.clear_txs();
        let fan_out_mem_block_handler = config
            .publish
            .map(|config| -> Result<MemPoolPublishService> {
                log::info!("Setup fan out mem_block handler.");
                let producer = tokio_kafka::Producer::connect(config.hosts, config.topic)?;
                let handler = MemPoolPublishService::start(producer);
                Ok(handler)
            })
            .transpose()?;

        let mem_pool_state = {
            let mem_store = MemStore::new(store.get_snapshot());
            Arc::new(MemPoolState::new(Arc::new(mem_store)))
        };

        let mut mem_pool = MemPool {
            store,
            current_tip: tip,
            generator,
            error_tx_handler,
            error_tx_receipt_notifier,
            pending,
            mem_block,
            provider,
            pending_deposits,
            restore_manager: restore_manager.clone(),
            pending_restored_tx_hashes,
            mem_pool_publish_service: fan_out_mem_block_handler,
            node_mode,
            mem_pool_state,
            dynamic_config_manager,
        };

        // update mem block info
        let snap = mem_pool.mem_pool_state().load();
        snap.update_mem_pool_block_info(mem_pool.mem_block.block_info())?;
        // set tip
        mem_pool.reset(None, Some(tip.0)).await?;
        // clear stored mem blocks
        tokio::spawn(async move {
            restore_manager.delete_before_one_hour();
        });

        Ok(mem_pool)
    }

    pub fn mem_block(&self) -> &MemBlock {
        &self.mem_block
    }

    pub fn mem_pool_state(&self) -> Arc<MemPoolState> {
        self.mem_pool_state.clone()
    }

    pub fn restore_manager(&self) -> &RestoreManager {
        &self.restore_manager
    }

    pub fn save_mem_block(&mut self) -> Result<()> {
        if !self.pending_restored_tx_hashes.is_empty() {
            log::warn!(
                "save mem block, but have pending restored txs from previous restored mem block"
            );

            self.mem_block.force_reinject_tx_hashes(
                Vec::from_iter(self.pending_restored_tx_hashes.clone()).as_slice(),
            );
        }

        self.restore_manager.save(self.mem_block())
    }

    pub fn save_mem_block_with_suffix(&mut self, suffix: &str) -> Result<()> {
        if !self.pending_restored_tx_hashes.is_empty() {
            log::warn!(
                "save mem block, but have pending restored txs from previous restored mem block"
            );

            self.mem_block.force_reinject_tx_hashes(
                Vec::from(self.pending_restored_tx_hashes.clone()).as_slice(),
            );
        }

        self.restore_manager
            .save_with_suffix(self.mem_block(), suffix)
    }

    pub fn set_provider(&mut self, provider: Box<dyn MemPoolProvider + Send + Sync>) {
        self.provider = provider;
    }

    pub fn is_mem_txs_full(&self, expect_slots: usize) -> bool {
        self.mem_block.txs().len().saturating_add(expect_slots) > MAX_MEM_BLOCK_TXS
    }

    pub fn pending_restored_tx_hashes(&mut self) -> &mut VecDeque<H256> {
        &mut self.pending_restored_tx_hashes
    }

    /// Push a layer2 tx into pool
    #[instrument(skip_all)]
    pub async fn push_transaction(&mut self, tx: L2Transaction) -> Result<()> {
        let db = self.store.begin_transaction();

        let snap = self.mem_pool_state.load();
        let state = snap.state()?;
        self.push_transaction_with_db(&db, &state, tx).await?;
        db.commit()?;
        Ok(())
    }

    /// Push a layer2 tx into pool
    #[instrument(skip_all, fields(tx_hash = %tx.hash().pack()))]
    async fn push_transaction_with_db(
        &mut self,
        db: &StoreTransaction,
        state: &(impl State + CodeStore),
        tx: L2Transaction,
    ) -> Result<()> {
        // check duplication
        let tx_hash: H256 = tx.raw().hash().into();
        if self.mem_block.txs_set().contains(&tx_hash) {
            return Err(anyhow!("duplicated tx"));
        }

        // reject if mem block is full
        // TODO: we can use the pool as a buffer
        if self.mem_block.txs().len() >= MAX_MEM_BLOCK_TXS {
            return Err(anyhow!(
                "Mem block is full, MAX_MEM_BLOCK_TXS: {}",
                MAX_MEM_BLOCK_TXS
            ));
        }

        // verification
        self.verify_tx(state, &tx)?;

        // instantly run tx in background & update local state
        let t = Instant::now();
        let tx_receipt = self.finalize_tx(db, tx.clone()).await?;
        log::debug!("[push tx] finalize tx time: {}ms", t.elapsed().as_millis());

        // save tx receipt in mem pool
        let post_state = tx_receipt.post_state();
        self.mem_block.push_tx(tx_hash, post_state);
        db.insert_mem_pool_transaction_receipt(&tx_hash, tx_receipt)?;

        // Add to pool
        let account_id: u32 = tx.raw().from_id().unpack();
        db.insert_mem_pool_transaction(&tx_hash, tx.clone())?;
        let entry_list = self.pending.entry(account_id).or_default();
        entry_list.txs.push(tx);

        Ok(())
    }

    /// verify tx
    #[instrument(skip_all)]
    fn verify_tx(&self, state: &(impl State + CodeStore), tx: &L2Transaction) -> Result<()> {
        // check tx size
        if tx.as_slice().len() > MAX_TX_SIZE {
            return Err(anyhow!("tx over size"));
        }

        // verify transaction
        self.generator.verify_transaction(state, tx)?;
        // verify signature
        self.generator.check_transaction_signature(state, tx)?;

        Ok(())
    }

    /// Push a withdrawal request into pool
    #[instrument(skip_all, fields(withdrawal = %withdrawal.hash().pack()))]
    pub async fn push_withdrawal_request(
        &mut self,
        withdrawal: WithdrawalRequestExtra,
    ) -> Result<()> {
        // check withdrawal size
        if withdrawal.as_slice().len() > MAX_WITHDRAWAL_SIZE {
            return Err(anyhow!("withdrawal over size"));
        }

        // check duplication
        let withdrawal_hash: H256 = withdrawal.raw().hash().into();
        if self.mem_block.withdrawals_set().contains(&withdrawal_hash) {
            return Err(anyhow!("duplicated withdrawal"));
        }

        // basic verification
        let snap = self.mem_pool_state.load();
        let state = snap.state()?;
        self.verify_withdrawal_request(&withdrawal, &state).await?;

        // Check replace-by-fee
        // TODO

        let account_script_hash: H256 = withdrawal.raw().account_script_hash().unpack();
        let account_id = state
            .get_account_id_by_script_hash(&account_script_hash)?
            .expect("get account_id");
        let entry_list = self.pending.entry(account_id).or_default();
        entry_list.withdrawals.push(withdrawal.clone());
        // Add to pool
        let db = self.store.begin_transaction();
        db.insert_mem_pool_withdrawal(&withdrawal_hash, withdrawal)?;
        db.commit()?;
        Ok(())
    }

    // Withdrawal request verification
    // TODO: duplicate withdrawal check
    #[instrument(skip_all)]
    async fn verify_withdrawal_request(
        &self,
        withdrawal: &WithdrawalRequestExtra,
        state: &(impl State + CodeStore),
    ) -> Result<()> {
        // check withdrawal size
        if withdrawal.as_slice().len() > MAX_WITHDRAWAL_SIZE {
            return Err(anyhow!("withdrawal over size"));
        }

        // verify withdrawal signature
        self.generator
            .check_withdrawal_request_signature(state, &withdrawal.request())?;

        // verify finalized custodian
        let finalized_custodians = {
            // query withdrawals from ckb-indexer
            let last_finalized_block_number = self
                .generator
                .rollup_context()
                .last_finalized_block_number(self.current_tip.1);
            self.provider
                .query_available_custodians(
                    vec![withdrawal.request()],
                    last_finalized_block_number,
                    self.generator.rollup_context().to_owned(),
                )
                .await?
        };
        let avaliable_custodians = AvailableCustodians::from(&finalized_custodians);
        let withdrawal_generator =
            WithdrawalGenerator::new(self.generator.rollup_context(), avaliable_custodians);
        withdrawal_generator.verify_remained_amount(&withdrawal.request())?;

        // withdrawal basic verification
        let db = self.store.begin_transaction();
        let asset_script = db.get_asset_script(&withdrawal.raw().sudt_script_hash().unpack())?;
        let unlock_withdrawl = UnlockWithdrawal::from(withdrawal);
        self.generator
            .verify_withdrawal_request(state, &withdrawal.request(), asset_script, unlock_withdrawl)
            .map_err(Into::into)
    }

    /// Return pending contents
    fn pending(&self) -> &HashMap<u32, EntryList> {
        &self.pending
    }

    /// Notify new tip
    /// this method update current state of mem pool
    #[instrument(skip_all)]
    pub async fn notify_new_tip(&mut self, new_tip: H256) -> Result<()> {
        // reset pool state
        self.reset(Some(self.current_tip.0), Some(new_tip)).await?;
        Ok(())
    }

    /// Clear mem block state and recollect deposits
    #[instrument(skip_all)]
    pub async fn reset_mem_block(&mut self) -> Result<()> {
        log::info!("[mem-pool] reset mem block");
        // reset pool state
        self.reset(Some(self.current_tip.0), Some(self.current_tip.0))
            .await?;
        Ok(())
    }

    /// FIXME
    /// This function is a temporary mechanism
    /// Try to recovery from invalid state by drop txs & deposit
    pub fn try_to_recovery_from_invalid_state(&mut self) -> Result<()> {
        log::warn!("[mem-pool] try to recovery from invalid state by drop txs & deposits");
        log::warn!("[mem-pool] drop mem-block");
        log::warn!(
            "[mem-pool] drop withdrawals: {}",
            self.mem_block.withdrawals().len()
        );
        log::warn!("[mem-pool] drop txs: {}", self.mem_block.txs().len());
        for tx_hash in self.mem_block.txs() {
            log::warn!("[mem-pool] drop tx: {}", hex::encode(tx_hash.as_slice()));
        }
        self.mem_block.clear();
        log::warn!("[mem-pool] drop pending: {}", self.pending.len());
        self.pending.clear();
        log::warn!("[mem-pool] try_to_recovery - done");
        Ok(())
    }

    /// output mem block
    #[instrument(skip_all, fields(retry_count = output_param.retry_count))]
    pub fn output_mem_block(&self, output_param: &OutputParam) -> (MemBlock, AccountMerkleState) {
        Self::package_mem_block(&self.mem_block, output_param)
    }

    pub(crate) fn package_mem_block(
        mem_block: &MemBlock,
        output_param: &OutputParam,
    ) -> (MemBlock, AccountMerkleState) {
        let (withdrawals_count, deposits_count, txs_count) =
            repackage_count(mem_block, output_param);

        log::info!(
            "[mem-pool] package mem block, retry count {}",
            output_param.retry_count
        );
        mem_block.repackage(withdrawals_count, deposits_count, txs_count)
    }

    /// Reset
    /// this method reset the current state of the mem pool
    /// discarded txs & withdrawals will be reinject to pool
    #[instrument(skip_all, fields(old_tip = old_tip.map(|h| display(h.pack())), new_tip = new_tip.map(|h| display(h.pack()))))]
    async fn reset(&mut self, old_tip: Option<H256>, new_tip: Option<H256>) -> Result<()> {
        let mut reinject_txs = Default::default();
        let mut reinject_withdrawals = Default::default();
        // read block from db
        let new_tip = match new_tip {
            Some(block_hash) => block_hash,
            None => {
                let db = self.store.begin_transaction();
                db.get_last_valid_tip_block_hash()?
            }
        };
        let new_tip_block = self.store.get_block(&new_tip)?.expect("new tip block");

        if old_tip.is_some() && old_tip != Some(new_tip_block.raw().parent_block_hash().unpack()) {
            let old_tip = old_tip.unwrap();
            let old_tip_block = self.store.get_block(&old_tip)?.expect("old tip block");

            let new_number: u64 = new_tip_block.raw().number().unpack();
            let old_number: u64 = old_tip_block.raw().number().unpack();
            let depth = max(new_number, old_number) - min(new_number, old_number);
            if depth > 64 {
                log::error!("skipping deep transaction reorg: depth {}", depth);
            } else {
                let mut rem = old_tip_block;
                let mut add = new_tip_block.clone();
                let mut discarded_txs: VecDeque<L2Transaction> = Default::default();
                let mut included_txs: HashSet<L2Transaction> = Default::default();
                let mut discarded_withdrawals: VecDeque<WithdrawalRequest> = Default::default();
                let mut included_withdrawals: HashSet<WithdrawalRequest> = Default::default();
                while rem.raw().number().unpack() > add.raw().number().unpack() {
                    // reverse push, so we can keep txs in block's order
                    for index in (0..rem.transactions().len()).rev() {
                        discarded_txs.push_front(rem.transactions().get(index).unwrap());
                    }
                    // reverse push, so we can keep withdrawals in block's order
                    for index in (0..rem.withdrawals().len()).rev() {
                        discarded_withdrawals.push_front(rem.withdrawals().get(index).unwrap());
                    }
                    rem = self
                        .store
                        .get_block(&rem.raw().parent_block_hash().unpack())?
                        .expect("get block");
                }
                while add.raw().number().unpack() > rem.raw().number().unpack() {
                    included_txs.extend(add.transactions().into_iter());
                    included_withdrawals.extend(rem.withdrawals().into_iter());
                    add = self
                        .store
                        .get_block(&add.raw().parent_block_hash().unpack())?
                        .expect("get block");
                }
                while rem.hash() != add.hash() {
                    // reverse push, so we can keep txs in block's order
                    for index in (0..rem.transactions().len()).rev() {
                        discarded_txs.push_front(rem.transactions().get(index).unwrap());
                    }
                    // reverse push, so we can keep withdrawals in block's order
                    for index in (0..rem.withdrawals().len()).rev() {
                        discarded_withdrawals.push_front(rem.withdrawals().get(index).unwrap());
                    }
                    rem = self
                        .store
                        .get_block(&rem.raw().parent_block_hash().unpack())?
                        .expect("get block");
                    included_txs.extend(add.transactions().into_iter());
                    included_withdrawals.extend(add.withdrawals().into_iter());
                    add = self
                        .store
                        .get_block(&add.raw().parent_block_hash().unpack())?
                        .expect("get block");
                }
                // remove included txs
                discarded_txs.retain(|tx| !included_txs.contains(tx));
                reinject_txs = discarded_txs;
                // remove included withdrawals
                discarded_withdrawals
                    .retain(|withdrawal| !included_withdrawals.contains(withdrawal));
                reinject_withdrawals = discarded_withdrawals
                    .into_iter()
                    .map(Into::<WithdrawalRequestExtra>::into)
                    .collect::<VecDeque<_>>()
            }
        }

        let db = self.store.begin_transaction();

        if self.node_mode != NodeMode::ReadOnly {
            // check pending deposits
            self.refresh_deposit_cells(&db, new_tip).await?;
        } else {
            self.pending_deposits.clear();
        }

        // estimate next l2block timestamp
        let estimated_timestamp = {
            let estimated = self.provider.estimate_next_blocktime().await?;
            let tip_timestamp = Duration::from_millis(new_tip_block.raw().timestamp().unpack());
            if estimated <= tip_timestamp {
                let overwriten_timestamp = tip_timestamp.saturating_add(Duration::from_secs(1));
                log::warn!(
                    "[mem-pool] reset mem-pool with insatisfied estimated time, overwrite estimated time {:?} -> {:?}",
                    estimated,
                    overwriten_timestamp
                );
                overwriten_timestamp
            } else {
                estimated
            }
        };
        // reset mem block state
        // Fix execute_raw_l2transaction panic by updating mem_store first and storing it to mem_pool_state after.
        let snapshot = self.store.get_snapshot();
        assert_eq!(snapshot.get_tip_block_hash()?, new_tip, "set new snapshot");
        let mem_store = MemStore::new(snapshot);
        let mem_block_content = self.mem_block.reset(&new_tip_block, estimated_timestamp);
        mem_store.update_mem_pool_block_info(self.mem_block.block_info())?;
        self.mem_pool_state.store(Arc::new(mem_store));

        // set tip
        self.current_tip = (new_tip, new_tip_block.raw().number().unpack());

        // mem block withdrawals
        let mem_block_withdrawals: Vec<_> = {
            let mut withdrawals = Vec::with_capacity(mem_block_content.withdrawals.len());
            for withdrawal_hash in mem_block_content.withdrawals {
                if let Some(withdrawal) = db.get_mem_pool_withdrawal(&withdrawal_hash)? {
                    withdrawals.push(withdrawal);
                }
            }
            withdrawals
        };

        // Process txs
        let mem_block_txs: Vec<_> = {
            let mut txs = Vec::with_capacity(mem_block_content.txs.len());
            for tx_hash in mem_block_content.txs {
                if let Some(tx) = db.get_mem_pool_transaction(&tx_hash)? {
                    txs.push(tx);
                }
            }
            txs
        };

        // remove from pending
        self.remove_unexecutables(&db).await?;

        log::info!("[mem-pool] reset reinject txs: {} mem-block txs: {} reinject withdrawals: {} mem-block withdrawals: {}", reinject_txs.len(), mem_block_txs.len(), reinject_withdrawals.len(), mem_block_withdrawals.len());
        // re-inject withdrawals
        let withdrawals_iter = reinject_withdrawals
            .into_iter()
            .chain(mem_block_withdrawals);
        // re-inject txs
        let txs_iter = reinject_txs.into_iter().chain(mem_block_txs);

        if self.node_mode != NodeMode::ReadOnly {
            self.prepare_next_mem_block(&db, withdrawals_iter, txs_iter)
                .await?;
        }
        db.commit()?;

        Ok(())
    }

    /// Discard unexecutables from pending.
    #[instrument(skip_all)]
    async fn remove_unexecutables(&mut self, db: &StoreTransaction) -> Result<()> {
        let snap = self.mem_pool_state.load();
        let state = snap.state()?;
        let mut remove_list = Vec::default();
        // iter pending accounts and demote any non-executable objects
        for (&account_id, list) in &mut self.pending {
            let nonce = state.get_nonce(account_id)?;

            // drop txs if tx.nonce lower than nonce
            let deprecated_txs = list.remove_lower_nonce_txs(nonce);
            for tx in deprecated_txs {
                let tx_hash = tx.hash().into();
                db.remove_mem_pool_transaction(&tx_hash)?;
            }
            // Drop all withdrawals that are have no enough balance
            let script_hash = state.get_script_hash(account_id)?;
            let capacity =
                state.get_sudt_balance(CKB_SUDT_ACCOUNT_ID, to_short_address(&script_hash))?;
            let deprecated_withdrawals = list.remove_lower_nonce_withdrawals(nonce, capacity);
            for withdrawal in deprecated_withdrawals {
                let withdrawal_hash: H256 = withdrawal.hash().into();
                db.remove_mem_pool_withdrawal(&withdrawal_hash)?;
            }
            // Delete empty entry
            if list.is_empty() {
                remove_list.push(account_id);
            }
        }
        for account_id in remove_list {
            self.pending.remove(&account_id);
        }
        Ok(())
    }

    /// Prepare for next mem block
    #[instrument(skip_all, fields(withdrawals_count = withdrawals.size_hint().1, txs_count = txs.size_hint().1))]
    async fn prepare_next_mem_block<
        WithdrawalIter: Iterator<Item = WithdrawalRequestExtra>,
        TxIter: Iterator<Item = L2Transaction> + Clone,
    >(
        &mut self,
        db: &StoreTransaction,
        withdrawals: WithdrawalIter,
        txs: TxIter,
    ) -> Result<()> {
        // check order of inputs
        {
            let mut id_to_nonce: HashMap<u32, u32> = HashMap::default();
            for tx in txs.clone() {
                let id: u32 = tx.raw().from_id().unpack();
                let nonce: u32 = tx.raw().nonce().unpack();
                if let Some(&prev_nonce) = id_to_nonce.get(&id) {
                    assert!(
                        nonce > prev_nonce,
                        "id: {} nonce({}) > prev_nonce({})",
                        id,
                        nonce,
                        prev_nonce
                    );
                }
                id_to_nonce.entry(id).or_insert(nonce);
            }
        }
        // Handle state before txs
        // withdrawal
        let withdrawals: Vec<WithdrawalRequestExtra> = withdrawals.collect();
        self.finalize_withdrawals(withdrawals.clone()).await?;
        // deposits
        let deposit_cells = self.pending_deposits.clone();
        self.finalize_deposits(deposit_cells.clone()).await?;

        // Fan-out next mem block to readonly node
        if let Some(handler) = &self.mem_pool_publish_service {
            let withdrawals = withdrawals.into_iter().map(|w| w.request()).collect();
            handler
                .next_mem_block(
                    withdrawals,
                    deposit_cells,
                    self.mem_block.block_info().clone(),
                )
                .await
        }

        // deposits
        let snap = self.mem_pool_state.load();
        let state = snap.state()?;

        // re-inject txs
        for tx in txs {
            if let Err(err) = self.push_transaction_with_db(db, &state, tx.clone()).await {
                let tx_hash = tx.hash();
                log::info!(
                    "[mem pool] fail to re-inject tx {}, error: {}",
                    hex::encode(&tx_hash),
                    err
                );
            }
        }

        Ok(())
    }

    /// expire if pending deposits is handled by new l2block
    #[instrument(skip_all)]
    async fn refresh_deposit_cells(
        &mut self,
        db: &StoreTransaction,
        new_block_hash: H256,
    ) -> Result<()> {
        // get processed deposit requests
        let processed_deposit_requests: HashSet<_> = db
            .get_block_deposit_requests(&new_block_hash)?
            .unwrap_or_default()
            .into_iter()
            .collect();

        // check expire
        let mut force_expired = false;
        let mut tasks = Vec::with_capacity(self.pending_deposits.len());
        for deposit in &self.pending_deposits {
            // check is handled by current block
            if processed_deposit_requests.contains(&deposit.request) {
                force_expired = true;
                break;
            }

            // query deposit live cell
            tasks.push(self.provider.get_cell(deposit.cell.out_point.clone()));
        }

        // check cell is available
        for task in tasks {
            match task.await? {
                Some(cell_with_status) => {
                    if cell_with_status.status != CellStatus::Live {
                        force_expired = true;
                        break;
                    }
                }
                None => {
                    force_expired = true;
                    break;
                }
            }
        }

        // refresh
        let snap = self.mem_pool_state.load();
        let state = snap.state()?;
        let mem_account_count = state.get_account_count()?;
        let tip_account_count: u32 = {
            let new_tip_block = db
                .get_block(&new_block_hash)?
                .ok_or_else(|| anyhow!("can't find new tip block"))?;
            new_tip_block.raw().post_account().count().unpack()
        };

        // we can safely expire pending deposits if the number of account doesn't change or mem block txs is empty
        // in these situation more deposits do not affects mem-pool account's id
        let safe_expired = self.pending_deposits.is_empty()
            && (mem_account_count == tip_account_count || self.mem_block.txs().is_empty());
        if safe_expired {
            log::debug!(
                    "[mem-pool] safely refresh pending deposits, mem_account_count: {}, tip_account_count: {}",
                    mem_account_count,
                    tip_account_count
                );
            let cells = self.provider.collect_deposit_cells().await?;
            self.pending_deposits = {
                let cells = cells
                    .into_iter()
                    .filter(|di| !processed_deposit_requests.contains(&di.request));
                crate::deposit::sanitize_deposit_cells(
                    self.generator.rollup_context(),
                    cells.collect(),
                )
            };
            log::debug!(
                "[mem-pool] refreshed deposits: {}",
                self.pending_deposits.len()
            );
        } else if force_expired {
            log::debug!(
                    "[mem-pool] forced clear pending deposits, mem_account_count: {}, tip_account_count: {}",
                    mem_account_count,
                    tip_account_count
                );
            self.pending_deposits.clear();
        } else {
            log::debug!(
                    "[mem-pool] skip pending deposits, pending deposits: {}, mem_account_count: {}, tip_account_count: {}",
                    self.pending_deposits.len(),
                    mem_account_count,
                    tip_account_count
                );
        }

        Ok(())
    }

    #[instrument(skip_all, fields(deposits_count = deposit_cells.len()))]
    async fn finalize_deposits(&mut self, deposit_cells: Vec<DepositInfo>) -> Result<()> {
        let snap = self.mem_pool_state.load();
        let mut state = snap.state()?;
        state.tracker_mut().enable();
        // update deposits
        let deposits: Vec<_> = deposit_cells.iter().map(|c| [c.request.clone()]).collect();
        let mut post_states = Vec::with_capacity(deposits.len());
        let mut touched_keys_vec = Vec::with_capacity(deposits.len());
        for deposit in deposits {
            state.apply_deposit_requests(self.generator.rollup_context(), &deposit)?;

            post_states.push(state.get_merkle_state());
            let touched_keys = state.tracker_mut().touched_keys().expect("touched keys");
            touched_keys_vec.push(touched_keys.lock().unwrap().drain().collect());
        }
        // calculate state after withdrawals & deposits
        let prev_state_checkpoint = state.calculate_state_checkpoint()?;
        log::debug!("[finalize deposits] deposits: {} state root: {}, account count: {}, prev_state_checkpoint {}",
         deposit_cells.len(), hex::encode(state.calculate_root()?.as_slice()), state.get_account_count()?, hex::encode(prev_state_checkpoint.as_slice()));

        self.mem_block.push_deposits(
            deposit_cells,
            post_states,
            touched_keys_vec,
            prev_state_checkpoint,
        );
        state.submit_tree_to_mem_block();
        Ok(())
    }

    /// Execute withdrawal & update local state
    #[instrument(skip_all, fields(withdrawals_count = withdrawals.len()))]
    async fn finalize_withdrawals(
        &mut self,
        mut withdrawals: Vec<WithdrawalRequestExtra>,
    ) -> Result<()> {
        // check mem block state
        assert!(self.mem_block.withdrawals().is_empty());
        assert!(self.mem_block.state_checkpoints().is_empty());
        assert!(self.mem_block.deposits().is_empty());
        assert!(self.mem_block.finalized_custodians().is_none());
        assert!(self.mem_block.txs().is_empty());

        // find withdrawals from pending
        if withdrawals.is_empty() {
            for entry in self.pending().values() {
                if !entry.withdrawals.is_empty() && withdrawals.len() < MAX_MEM_BLOCK_WITHDRAWALS {
                    withdrawals.push(entry.withdrawals.first().unwrap().clone());
                }
            }
        }

        let max_withdrawal_capacity = std::u128::MAX;
        let finalized_custodians = {
            // query withdrawals from ckb-indexer
            let last_finalized_block_number = self
                .generator
                .rollup_context()
                .last_finalized_block_number(self.current_tip.1);
            self.provider
                .query_available_custodians(
                    withdrawals.iter().map(|w| w.request()).collect(),
                    last_finalized_block_number,
                    self.generator.rollup_context().to_owned(),
                )
                .await?
        };

        let available_custodians = AvailableCustodians::from(&finalized_custodians);
        let asset_scripts: HashMap<H256, Script> = {
            let sudt_value = available_custodians.sudt.values();
            sudt_value.map(|(_, script)| (script.hash().into(), script.to_owned()))
        }
        .collect();
        let snap = self.mem_pool_state.load();
        let mut state = snap.state()?;
        // verify the withdrawals
        let mut unused_withdrawals = Vec::with_capacity(withdrawals.len());
        let mut total_withdrawal_capacity: u128 = 0;
        let mut withdrawal_verifier = crate::withdrawal::Generator::new(
            self.generator.rollup_context(),
            available_custodians,
        );
        // start track withdrawal
        state.tracker_mut().enable();
        for withdrawal in withdrawals {
            let withdrawal_hash = withdrawal.hash();
            // check withdrawal request
            if let Err(err) = self
                .generator
                .check_withdrawal_request_signature(&state, &withdrawal.request())
            {
                log::info!("[mem-pool] withdrawal signature error: {:?}", err);
                unused_withdrawals.push(withdrawal_hash);
                continue;
            }
            let asset_script = asset_scripts
                .get(&withdrawal.raw().sudt_script_hash().unpack())
                .cloned();
            if let Err(err) = self.generator.verify_withdrawal_request(
                &state,
                &withdrawal.request(),
                asset_script,
                UnlockWithdrawal::from(&withdrawal),
            ) {
                log::info!("[mem-pool] withdrawal verification error: {:?}", err);
                unused_withdrawals.push(withdrawal_hash);
                continue;
            }
            let capacity: u64 = withdrawal.raw().capacity().unpack();
            let new_total_withdrwal_capacity = total_withdrawal_capacity
                .checked_add(capacity as u128)
                .ok_or_else(|| anyhow!("total withdrawal capacity overflow"))?;
            // skip package withdrwal if overdraft the Rollup capacity
            if new_total_withdrwal_capacity > max_withdrawal_capacity {
                log::info!(
                    "[mem-pool] max_withdrawal_capacity({}) is not enough to withdraw({})",
                    max_withdrawal_capacity,
                    new_total_withdrwal_capacity
                );
                unused_withdrawals.push(withdrawal_hash);
                continue;
            }
            total_withdrawal_capacity = new_total_withdrwal_capacity;

            if let Err(err) =
                withdrawal_verifier.include_and_verify(&withdrawal, &L2Block::default())
            {
                log::info!(
                    "[mem-pool] withdrawal contextual verification failed : {}",
                    err
                );
                unused_withdrawals.push(withdrawal_hash);
                continue;
            }

            // update the state
            match state.apply_withdrawal_request(
                self.generator.rollup_context(),
                self.mem_block.block_producer_id(),
                &withdrawal.request(),
            ) {
                Ok(_) => {
                    let post_state = state.get_merkle_state();
                    let touched_keys = state.tracker_mut().touched_keys().expect("touched keys");

                    self.mem_block.push_withdrawal(
                        withdrawal.hash().into(),
                        post_state,
                        touched_keys.lock().unwrap().drain(),
                    );
                }
                Err(err) => {
                    log::info!("[mem-pool] withdrawal execution failed : {}", err);
                    unused_withdrawals.push(withdrawal_hash);
                }
            }
        }
        state.submit_tree_to_mem_block();
        self.mem_block
            .set_finalized_custodians(finalized_custodians);

        // remove unused withdrawals
        log::info!(
            "[mem-pool] finalize withdrawals: {} staled withdrawals: {}",
            self.mem_block.withdrawals().len(),
            unused_withdrawals.len()
        );
        Ok(())
    }

    /// Execute tx & update local state
    #[instrument(skip_all)]
    async fn finalize_tx(&mut self, db: &StoreTransaction, tx: L2Transaction) -> Result<TxReceipt> {
        let snap = self.mem_pool_state.load();
        let mut state = snap.state()?;
        let tip_block_hash = db.get_tip_block_hash()?;
        let chain_view = ChainView::new(db, tip_block_hash);

        let block_info = self.mem_block.block_info();

        // execute tx
        let raw_tx = tx.raw();
        let run_result = self.generator.unchecked_execute_transaction(
            &chain_view,
            &state,
            block_info,
            &raw_tx,
            L2TX_MAX_CYCLES,
            Some(self.dynamic_config_manager.clone()),
        )?;

        if run_result.exit_code != 0 {
            let tx_hash: H256 = tx.hash().into();
            let block_number = self.mem_block.block_info().number().unpack();

            let receipt = ErrorTxReceipt {
                tx_hash,
                block_number,
                return_data: run_result.return_data,
                last_log: run_result.logs.last().cloned(),
                exit_code: run_result.exit_code,
            };
            if let Some(ref mut error_tx_handler) = self.error_tx_handler {
                let t = Instant::now();
                if let Err(err) = error_tx_handler.handle_error_receipt(receipt.clone()).await {
                    log::warn!("handle error receipt {}", err);
                }
                log::debug!(
                    "[finalize tx] handle error tx: {}ms",
                    t.elapsed().as_millis()
                );
            }
            if let Some(notifier) = self.error_tx_receipt_notifier.as_ref() {
                notifier.notify_new_error_tx_receipt(receipt);
            }

            return Err(TransactionError::InvalidExitCode(run_result.exit_code).into());
        }

        // apply run result
        let t = Instant::now();
        state.apply_run_result(&run_result)?;
        log::debug!(
            "[finalize tx] apply run result: {}ms",
            t.elapsed().as_millis()
        );
        let t = Instant::now();
        state.submit_tree_to_mem_block();
        log::debug!(
            "[finalize tx] submit tree to mem_block: {}ms",
            t.elapsed().as_millis()
        );

        // generate tx receipt
        let merkle_state = state.merkle_state()?;
        let tx_receipt =
            TxReceipt::build_receipt(tx.witness_hash().into(), run_result, merkle_state);

        // fan-out to readonly mem block
        if let Some(handler) = &self.mem_pool_publish_service {
            handler.new_tx(tx, self.current_tip.1).await
        }

        Ok(tx_receipt)
    }

    // Only **ReadOnly** node needs this.
    // Refresh mem block with those params.
    // Always expects next block number equals with current_tip_block_number + 1.
    // This function returns Ok(Some(block_number)), if refresh is successful.
    // Or returns Ok(None) if current tip has not synced yet.
    #[instrument(skip_all, fields(block = block_info.number().unpack(), withdrawals_count = withdrawals.len(), deposits_count = deposits.len()))]
    pub(crate) async fn refresh_mem_block(
        &mut self,
        block_info: BlockInfo,
        withdrawals: Vec<WithdrawalRequest>,
        deposits: Vec<DepositInfo>,
    ) -> Result<Option<u64>> {
        let next_block_number = block_info.number().unpack();
        let current_tip_block_number = self.current_tip.1;
        if next_block_number <= current_tip_block_number {
            // mem blocks from the past should be ignored
            log::trace!(
                "Ignore this mem block: {}, current tip: {}",
                next_block_number,
                current_tip_block_number
            );
            return Ok(Some(current_tip_block_number));
        }
        if next_block_number != current_tip_block_number + 1 {
            return Ok(None);
        }
        let db = self.store.begin_transaction();
        let tip_block = db.get_last_valid_tip_block()?;

        let post_merkle_state = tip_block.raw().post_account();
        let mem_block = MemBlock::new(block_info, post_merkle_state);
        self.mem_block = mem_block;

        let withdrawals = withdrawals.into_iter().map(Into::into).collect();
        self.finalize_withdrawals(withdrawals).await?;
        self.finalize_deposits(deposits).await?;

        db.commit()?;

        let mem_block = &self.mem_block;
        log::info!(
            "Refreshed mem_block: block id: {}, deposits: {}, withdrawals: {}, txs: {}",
            mem_block.block_info().number().unpack(),
            mem_block.deposits().len(),
            mem_block.withdrawals().len(),
            mem_block.txs().len()
        );

        Ok(Some(next_block_number))
    }

    // Only **ReadOnly** node needs this.
    // Sync tx from fullnode to readonly.
    #[instrument(skip_all, fields(tx_hash = %tx.hash().pack(), current_tip_block = current_tip_block_number))]
    pub(crate) async fn append_tx(
        &mut self,
        tx: L2Transaction,
        current_tip_block_number: u64,
    ) -> Result<()> {
        // Always expects tx from current tip.
        // Ignore tx from an old block.
        if current_tip_block_number < self.current_tip.1 {
            // txs from the past block should be ignored
            return Ok(());
        }
        self.push_transaction(tx).await?;
        Ok(())
    }
}

pub(crate) fn repackage_count(
    mem_block: &MemBlock,
    output_param: &OutputParam,
) -> (usize, usize, usize) {
    let total = mem_block.withdrawals().len() + mem_block.deposits().len() + mem_block.txs().len();
    // Drop base on retry count
    let mut remain = total.shr(output_param.retry_count);
    if 0 == remain {
        // Package at least one
        remain = 1;
    }

    let withdrawals_count = mem_block.withdrawals().iter().take(remain).count();
    remain = remain.saturating_sub(withdrawals_count);

    let deposits_count = mem_block.deposits().iter().take(remain).count();
    remain = remain.saturating_sub(deposits_count);

    let txs_count = mem_block.txs().iter().take(remain).count();

    (withdrawals_count, deposits_count, txs_count)
}

#[cfg(test)]
mod test {
    use std::ops::Shr;

    use gw_common::merkle_utils::calculate_state_checkpoint;
    use gw_common::H256;
    use gw_types::offchain::{CollectedCustodianCells, DepositInfo};
    use gw_types::packed::{AccountMerkleState, BlockInfo, DepositRequest};
    use gw_types::prelude::{Builder, Entity, Pack, Unpack};

    use crate::mem_block::{MemBlock, MemBlockCmp};
    use crate::pool::{repackage_count, MemPool, OutputParam};

    #[test]
    fn test_package_mem_block() {
        let block_info = BlockInfo::new_builder()
            .block_producer_id(1u32.pack())
            .build();
        let prev_merkle_state = AccountMerkleState::new_builder().count(3u32.pack()).build();

        // Random withdrawals
        let withdrawals_count = 50;
        let withdrawals: Vec<_> = (0..withdrawals_count).map(|_| random_hash()).collect();
        let withdrawals_touch_keys: Vec<_> =
            { (0..withdrawals_count).map(|_| vec![random_hash()]) }.collect();
        let withdrawals_state: Vec<_> = (0..withdrawals_count).map(|_| random_state()).collect();

        // Random deposits
        let deposits_count = 50;
        let deposits: Vec<_> = {
            (0..deposits_count).map(|_| DepositInfo {
                request: DepositRequest::new_builder()
                    .sudt_script_hash(random_hash().pack())
                    .build(),
                ..Default::default()
            })
        }
        .collect();
        let deposits_touch_keys: Vec<_> =
            (0..deposits_count).map(|_| vec![random_hash()]).collect();
        let deposits_state: Vec<_> = { (0..deposits_count).map(|_| random_state()) }.collect();

        let finalized_custodians = CollectedCustodianCells {
            capacity: rand::random::<u128>(),
            ..Default::default()
        };

        // Random txs
        let txs_count = 500;
        let txs: Vec<_> = (0..txs_count).map(|_| random_hash()).collect();
        let txs_state: Vec<_> = (0..txs_count).map(|_| random_state()).collect();

        // Fill mem block
        let mem_block = {
            let mut mem_block = MemBlock::new(block_info.clone(), prev_merkle_state.clone());
            for ((hash, touched_keys), state) in { withdrawals.clone().into_iter() }
                .zip(withdrawals_touch_keys.clone())
                .zip(withdrawals_state.clone())
            {
                mem_block.push_withdrawal(hash, state, touched_keys.into_iter());
            }
            mem_block.set_finalized_custodians(finalized_custodians.clone());

            let txs_prev_state_checkpoint = {
                let state = deposits_state.last().unwrap();
                calculate_state_checkpoint(&state.merkle_root().unpack(), state.count().unpack())
            };
            mem_block.push_deposits(
                deposits.clone(),
                deposits_state.clone(),
                deposits_touch_keys.clone(),
                txs_prev_state_checkpoint,
            );
            for (hash, state) in txs.clone().into_iter().zip(txs_state.clone()) {
                mem_block.push_tx(hash, state);
            }

            mem_block
        };

        // Retry count 0, package whole mem block
        let (mem_block_out, post_block_state) =
            MemPool::package_mem_block(&mem_block, &OutputParam { retry_count: 0 });
        let expected_block = &mem_block;

        // Check output mem block
        assert_eq!(mem_block_out.cmp(expected_block), MemBlockCmp::Same);
        assert_eq!(
            &post_block_state,
            mem_block.tx_post_states().last().unwrap()
        );

        let repackage = |withdrawals_count, deposits_count, txs_count| -> _ {
            let mut expected = MemBlock::new(block_info.clone(), prev_merkle_state.clone());
            let mut post_states = vec![prev_merkle_state.clone()];
            for ((hash, touched_keys), state) in { withdrawals.clone().into_iter() }
                .zip(withdrawals_touch_keys.clone())
                .zip(withdrawals_state.clone())
                .take(withdrawals_count)
            {
                expected.push_withdrawal(hash, state.clone(), touched_keys);
                post_states.push(state);
            }
            let deposits = deposits.iter().cloned().take(deposits_count).collect();
            let deposit_states: Vec<_> =
                { deposits_state.clone().into_iter().take(deposits_count) }.collect();
            let deposit_touched_keys =
                { deposits_touch_keys.clone().into_iter().take(deposits_count) }.collect();

            post_states.extend(deposit_states.clone());
            let txs_prev_state_checkpoint = {
                let state = post_states.last().unwrap();
                calculate_state_checkpoint(&state.merkle_root().unpack(), state.count().unpack())
            };
            expected.push_deposits(
                deposits,
                deposit_states,
                deposit_touched_keys,
                txs_prev_state_checkpoint,
            );

            for (hash, state) in { txs.clone().into_iter() }
                .zip(txs_state.clone())
                .take(txs_count)
            {
                expected.push_tx(hash, state.clone());
                post_states.push(state);
            }

            expected.set_finalized_custodians(finalized_custodians.clone());

            (expected, post_states.last().unwrap().to_owned())
        };

        // Retry count 1, should remove half of packaged state changes
        let total =
            mem_block.withdrawals().len() + mem_block.deposits().len() + mem_block.txs().len();
        let remain = total.shr(1);
        assert!(remain > 0usize);

        let output_param = OutputParam { retry_count: 1 };
        let (mem_block_out, post_block_state) =
            MemPool::package_mem_block(&mem_block, &output_param);

        let (withdrawals_count, deposits_count, txs_count) =
            repackage_count(&mem_block, &output_param);
        assert!(txs_count > 0);

        let (expected_block, expected_post_state) =
            repackage(withdrawals_count, deposits_count, txs_count);

        assert_eq!(mem_block_out.cmp(&expected_block), MemBlockCmp::Same);
        assert_eq!(post_block_state, expected_post_state);

        // Retry count 2
        let remain = total.shr(2);
        assert!(remain > 0usize);

        let output_param = OutputParam { retry_count: 2 };
        let (mem_block_out, post_block_state) =
            MemPool::package_mem_block(&mem_block, &output_param);

        let (withdrawals_count, deposits_count, txs_count) =
            repackage_count(&mem_block, &output_param);
        assert!(txs_count > 0);

        let (expected_block, expected_post_state) =
            repackage(withdrawals_count, deposits_count, txs_count);

        assert_eq!(mem_block_out.cmp(&expected_block), MemBlockCmp::Same);
        assert_eq!(post_block_state, expected_post_state);

        // Retry count 3
        let remain = total.shr(3);
        assert!(remain > 0usize);

        let output_param = OutputParam { retry_count: 3 };
        let (mem_block_out, post_block_state) =
            MemPool::package_mem_block(&mem_block, &output_param);

        let (withdrawals_count, deposits_count, txs_count) =
            repackage_count(&mem_block, &output_param);
        assert_eq!(txs_count, 0);
        assert!(deposits_count > 0);

        let (expected_block, expected_post_state) =
            repackage(withdrawals_count, deposits_count, txs_count);

        assert_eq!(mem_block_out.cmp(&expected_block), MemBlockCmp::Same);
        assert_eq!(post_block_state, expected_post_state);

        // Retry count 4 ~ 9
        for retry_count in 4..=9 {
            let remain = total.shr(retry_count);
            assert!(remain > 0usize);

            let output_param = OutputParam { retry_count };
            let (mem_block_out, post_block_state) =
                MemPool::package_mem_block(&mem_block, &output_param);

            let (withdrawals_count, deposits_count, txs_count) =
                repackage_count(&mem_block, &output_param);
            assert_eq!(txs_count, 0);
            assert_eq!(deposits_count, 0);
            assert!(withdrawals_count > 0);

            let (expected_block, expected_post_state) =
                repackage(withdrawals_count, deposits_count, txs_count);

            assert_eq!(mem_block_out.cmp(&expected_block), MemBlockCmp::Same);
            assert_eq!(post_block_state, expected_post_state);
        }

        // Retry count 10
        let remain = total.shr(10);
        assert_eq!(remain, 0usize);

        let output_param = OutputParam { retry_count: 10 };
        let (mem_block_out, post_block_state) =
            MemPool::package_mem_block(&mem_block, &output_param);

        let (withdrawals_count, deposits_count, txs_count) =
            repackage_count(&mem_block, &output_param);
        assert_eq!(txs_count, 0);
        assert_eq!(deposits_count, 0);
        assert_eq!(withdrawals_count, 1);

        // Should package at least one
        let (expected_block, expected_post_state) =
            repackage(withdrawals_count, deposits_count, txs_count);

        assert_eq!(mem_block_out.cmp(&expected_block), MemBlockCmp::Same);
        assert_eq!(post_block_state, expected_post_state);
    }

    fn random_hash() -> H256 {
        rand::random::<[u8; 32]>().into()
    }

    fn random_state() -> AccountMerkleState {
        AccountMerkleState::new_builder()
            .merkle_root(random_hash().pack())
            .count(rand::random::<u32>().pack())
            .build()
    }
}
