#![allow(clippy::mutable_key_type)]
#![allow(clippy::unnecessary_unwrap)]
//! MemPool
//!
//! The mem pool will update txs & withdrawals 'instantly' by running background tasks.
//! So a user could query the tx receipt 'instantly'.
//! Since we already got the next block status, the block prodcuer would not need to execute
//! txs & withdrawals again.
//!

use anyhow::{anyhow, bail, Result};
use arc_swap::ArcSwap;
use gw_challenge::offchain::{
    OffChainCancelChallengeValidator, OffChainValidatorContext, RollBackSavePointError,
};
use gw_common::{
    builtins::CKB_SUDT_ACCOUNT_ID,
    state::{to_short_address, State},
    H256,
};
use gw_config::MemPoolConfig;
use gw_generator::{
    constants::L2TX_MAX_CYCLES, error::TransactionError, traits::StateExt, Generator,
};
use gw_store::{
    chain_view::ChainView,
    state_db::{CheckPoint, StateDBMode, StateDBTransaction, SubState, WriteContext},
    transaction::{mem_pool_store::MemPoolStore, StoreTransaction},
    Store,
};
use gw_types::{
    offchain::{BlockParam, CollectedCustodianCells, DepositInfo, ErrorTxReceipt, RunResult},
    packed::{
        AccountMerkleState, BlockInfo, L2Block, L2Transaction, RawL2Transaction, Script, TxReceipt,
        WithdrawalRequest,
    },
    prelude::{Entity, Pack, Unpack},
};
use std::{
    cmp::{max, min},
    collections::{HashMap, HashSet, VecDeque},
    sync::Arc,
};

use crate::{
    constants::{MAX_MEM_BLOCK_TXS, MAX_MEM_BLOCK_WITHDRAWALS, MAX_TX_SIZE, MAX_WITHDRAWAL_SIZE},
    custodian::AvailableCustodians,
    mem_block::MemBlock,
    traits::{MemPoolErrorTxHandler, MemPoolProvider},
    types::EntryList,
    withdrawal::Generator as WithdrawalGenerator,
};

pub enum MemBlockDBMode {
    NewBlock,
    Package,
}

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

/// Shared mem pool logic
#[derive(Clone)]
pub struct Inner {
    /// store
    store: Store,
    /// current tip
    current_tip: Arc<ArcSwap<(H256, u64)>>,
    /// generator instance
    generator: Arc<Generator>,
    /// Mem pool config
    config: Arc<MemPoolConfig>,
}

impl Inner {
    pub fn new(
        store: Store,
        tip: (H256, u64),
        generator: Arc<Generator>,
        config: Arc<MemPoolConfig>,
    ) -> Self {
        let current_tip = Arc::new(ArcSwap::new(Arc::new(tip)));

        Inner {
            store,
            current_tip,
            generator,
            config,
        }
    }

    pub fn current_tip(&self) -> (H256, u64) {
        *self.current_tip.load_full()
    }

    pub fn set_current_tip(&self, tip: (H256, u64)) {
        self.current_tip.swap(Arc::new(tip));
    }

    pub fn fetch_state_db<'a>(&self, db: &'a StoreTransaction) -> Result<StateDBTransaction<'a>> {
        self.fetch_state_db_with_mode(db, MemBlockDBMode::NewBlock)
    }

    pub fn fetch_state_db_with_mode<'a>(
        &self,
        db: &'a StoreTransaction,
        mode: MemBlockDBMode,
    ) -> Result<StateDBTransaction<'a>> {
        // Repackage offset must be smaller than NewBlock, so that it has clean state.
        let db_mode = match mode {
            MemBlockDBMode::NewBlock => StateDBMode::Write(WriteContext::new(u32::MAX)),
            MemBlockDBMode::Package => StateDBMode::Write(WriteContext::new(0)),
        };
        StateDBTransaction::from_checkpoint(
            db,
            CheckPoint::new(self.current_tip().1, SubState::MemBlock),
            db_mode,
        )
        .map_err(|err| anyhow!("err: {}", err))
    }

    /// Execute tx without push it into pool and check exit code
    pub fn unchecked_execute_transaction(
        &self,
        tx: &L2Transaction,
        block_info: &BlockInfo,
    ) -> Result<RunResult> {
        let db = self.store.begin_transaction();
        let state_db = self.fetch_state_db(&db)?;
        let state = state_db.state_tree()?;
        let tip_block_hash = self.store.get_tip_block_hash()?;
        let chain_view = ChainView::new(&db, tip_block_hash);
        // verify tx signature
        self.generator.check_transaction_signature(&state, tx)?;
        // tx basic verification
        self.generator.verify_transaction(&state, tx)?;
        // execute tx
        let raw_tx = tx.raw();
        let run_result = self.generator.unchecked_execute_transaction(
            &chain_view,
            &state,
            block_info,
            &raw_tx,
            self.config.execute_l2tx_max_cycles,
        )?;
        Ok(run_result)
    }
}

/// MemPool
pub struct MemPool {
    /// Inner common logic
    inner: Inner,
    /// Store
    store: Store,
    /// Generator
    generator: Arc<Generator>,
    /// error tx handler,
    error_tx_handler: Option<Box<dyn MemPoolErrorTxHandler + Send>>,
    /// pending queue, contains executable contents(can be pacakged into block)
    pending: HashMap<u32, EntryList>,
    /// memory block
    mem_block: MemBlock,
    /// Mem pool provider
    provider: Box<dyn MemPoolProvider + Send>,
    /// Offchain cancel challenge validator
    offchain_validator: Option<OffChainCancelChallengeValidator>,
    /// Config
    config: Arc<MemPoolConfig>,
}

impl MemPool {
    pub fn create(
        store: Store,
        generator: Arc<Generator>,
        provider: Box<dyn MemPoolProvider + Send>,
        error_tx_handler: Option<Box<dyn MemPoolErrorTxHandler + Send>>,
        offchain_validator_context: Option<OffChainValidatorContext>,
        config: MemPoolConfig,
    ) -> Result<Self> {
        let pending = Default::default();

        let tip_block = store.get_tip_block()?;
        let tip = (tip_block.hash().into(), tip_block.raw().number().unpack());

        let config = Arc::new(config);
        let inner = Inner::new(
            store.clone(),
            tip,
            Arc::clone(&generator),
            Arc::clone(&config),
        );

        let mem_block: MemBlock = Default::default();
        let reverted_block_root = {
            let db = store.begin_transaction();
            let smt = db.reverted_block_smt()?;
            smt.root().to_owned()
        };
        let offchain_validator = offchain_validator_context.map(|offchain_validator_context| {
            OffChainCancelChallengeValidator::new(
                offchain_validator_context,
                mem_block.block_producer_id().pack(),
                &tip_block,
                mem_block.block_info().timestamp().unpack(),
                reverted_block_root,
            )
        });

        let mut mem_pool = MemPool {
            inner,
            store,
            generator,
            error_tx_handler,
            pending,
            mem_block,
            provider,
            offchain_validator,
            config,
        };

        // set tip
        mem_pool.reset(None, Some(tip.0))?;
        Ok(mem_pool)
    }

    pub fn mem_block(&self) -> &MemBlock {
        &self.mem_block
    }

    pub fn current_tip(&self) -> (H256, u64) {
        self.inner.current_tip()
    }

    pub fn inner(&self) -> Inner {
        self.inner.clone()
    }

    pub fn set_provider(&mut self, provider: Box<dyn MemPoolProvider + Send>) {
        self.provider = provider;
    }

    pub fn fetch_state_db<'a>(&self, db: &'a StoreTransaction) -> Result<StateDBTransaction<'a>> {
        self.inner.fetch_state_db(db)
    }

    pub fn fetch_state_db_with_mode<'a>(
        &self,
        db: &'a StoreTransaction,
        mode: MemBlockDBMode,
    ) -> Result<StateDBTransaction<'a>> {
        self.inner.fetch_state_db_with_mode(db, mode)
    }

    /// Push a layer2 tx into pool
    pub fn push_transaction(&mut self, tx: L2Transaction) -> Result<()> {
        let db = self.store.begin_transaction();
        self.push_transaction_with_db(&db, tx)?;
        db.commit()?;
        Ok(())
    }

    /// Push a layer2 tx into pool
    fn push_transaction_with_db(&mut self, db: &StoreTransaction, tx: L2Transaction) -> Result<()> {
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
        self.verify_tx(db, &tx)?;

        // instantly run tx in background & update local state
        let tx_receipt = self.finalize_tx(db, tx.clone())?;

        // save tx receipt in mem pool
        self.mem_block.push_tx(tx_hash, &tx_receipt);
        db.insert_mem_pool_transaction_receipt(&tx_hash, tx_receipt)?;

        // Add to pool
        let account_id: u32 = tx.raw().from_id().unpack();
        db.insert_mem_pool_transaction(&tx_hash, tx.clone())?;
        let entry_list = self.pending.entry(account_id).or_default();
        entry_list.txs.push(tx);

        Ok(())
    }

    /// verify tx
    fn verify_tx(&self, db: &StoreTransaction, tx: &L2Transaction) -> Result<()> {
        // check tx size
        if tx.as_slice().len() > MAX_TX_SIZE {
            return Err(anyhow!("tx over size"));
        }

        let state_db = self.fetch_state_db(db)?;
        let state = state_db.state_tree()?;
        // verify transaction
        self.generator.verify_transaction(&state, tx)?;
        // verify signature
        self.generator.check_transaction_signature(&state, tx)?;

        Ok(())
    }

    /// Execute tx without: a) push it into pool; 2) verify signature; 3) check nonce
    pub fn execute_raw_transaction(
        &self,
        raw_tx: RawL2Transaction,
        block_info: &BlockInfo,
        block_number_opt: Option<u64>,
    ) -> Result<RunResult> {
        let db = self.store.begin_transaction();
        let state_db = match block_number_opt {
            Some(block_number) => {
                let check_point = CheckPoint::new(block_number, SubState::Block);
                StateDBTransaction::from_checkpoint(&db, check_point, StateDBMode::ReadOnly)?
            }
            None => self.fetch_state_db(&db)?,
        };
        let state = state_db.state_tree()?;
        let tip_block_hash = self.store.get_tip_block_hash()?;
        let chain_view = ChainView::new(&db, tip_block_hash);
        // execute tx
        let run_result = self.generator.execute_transaction(
            &chain_view,
            &state,
            block_info,
            &raw_tx,
            self.config.execute_l2tx_max_cycles,
        )?;
        Ok(run_result)
    }

    /// Push a withdrawal request into pool
    pub fn push_withdrawal_request(&mut self, withdrawal: WithdrawalRequest) -> Result<()> {
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
        self.verify_withdrawal_request(&withdrawal)?;

        // Check replace-by-fee
        // TODO

        let db = self.store.begin_transaction();
        let state_db = self.fetch_state_db(&db)?;
        let state = state_db.state_tree()?;
        let account_script_hash: H256 = withdrawal.raw().account_script_hash().unpack();
        let account_id = state
            .get_account_id_by_script_hash(&account_script_hash)?
            .expect("get account_id");
        let entry_list = self.pending.entry(account_id).or_default();
        entry_list.withdrawals.push(withdrawal.clone());
        // Add to pool
        db.insert_mem_pool_withdrawal(&withdrawal_hash, withdrawal)?;
        db.commit()?;
        Ok(())
    }

    /// Verify withdrawal request without push it into pool
    pub fn verify_withdrawal_request(&self, withdrawal_request: &WithdrawalRequest) -> Result<()> {
        let db = self.store.begin_transaction();
        let state_db = self.fetch_state_db(&db)?;
        let state = state_db.state_tree()?;
        // verify withdrawal signature
        self.generator
            .check_withdrawal_request_signature(&state, withdrawal_request)?;

        // verify finalized custodian
        let finalized_custodians = {
            // query withdrawals from ckb-indexer
            let last_finalized_block_number = self
                .generator
                .rollup_context()
                .last_finalized_block_number(self.current_tip().1);
            let task = self.provider.query_available_custodians(
                vec![withdrawal_request.clone()],
                last_finalized_block_number,
                self.generator.rollup_context().to_owned(),
            );
            smol::block_on(task)?
        };
        let avaliable_custodians = AvailableCustodians::from(&finalized_custodians);
        let withdrawal_generator =
            WithdrawalGenerator::new(self.generator.rollup_context(), avaliable_custodians);
        withdrawal_generator.verify_remained_amount(withdrawal_request)?;

        // withdrawal basic verification
        let asset_script =
            db.get_asset_script(&withdrawal_request.raw().sudt_script_hash().unpack())?;
        self.generator
            .verify_withdrawal_request(&state, withdrawal_request, asset_script)
            .map_err(Into::into)
    }

    /// Return pending contents
    fn pending(&self) -> &HashMap<u32, EntryList> {
        &self.pending
    }

    /// Notify new tip
    /// this method update current state of mem pool
    pub fn notify_new_tip(&mut self, new_tip: H256) -> Result<()> {
        // reset pool state
        self.reset(Some(self.current_tip().0), Some(new_tip))?;
        Ok(())
    }

    /// Clear mem block state and recollect deposits
    pub fn reset_mem_block(&mut self) -> Result<()> {
        log::info!("[mem-pool] reset mem block");
        // reset pool state
        self.reset(Some(self.current_tip().0), Some(self.current_tip().0))?;
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
    pub fn output_mem_block(
        &self,
        output_param: &OutputParam,
    ) -> Result<(Option<CollectedCustodianCells>, BlockParam)> {
        let (mem_block, post_merkle_state) = self.package_mem_block(output_param)?;

        let db = self.store.begin_transaction();
        let state_db = StateDBTransaction::from_checkpoint(
            &db,
            CheckPoint::new(self.current_tip().1, SubState::Block),
            StateDBMode::ReadOnly,
        )?;
        // generate kv state & merkle proof from tip state
        let state = state_db.state_tree()?;

        let kv_state: Vec<(H256, H256)> = mem_block
            .touched_keys()
            .iter()
            .map(|k| {
                state
                    .get_raw(k)
                    .map(|v| (*k, v))
                    .map_err(|err| anyhow!("can't fetch value error: {:?}", err))
            })
            .collect::<Result<_>>()?;
        let kv_state_proof = if kv_state.is_empty() {
            // nothing need to prove
            Vec::new()
        } else {
            let account_smt = state_db.account_smt()?;

            account_smt
                .merkle_proof(kv_state.iter().map(|(k, _v)| *k).collect())
                .map_err(|err| anyhow!("merkle proof error: {:?}", err))?
                .compile(kv_state.clone())?
                .0
        };

        let txs = mem_block
            .txs()
            .iter()
            .map(|tx_hash| {
                db.get_mem_pool_transaction(tx_hash)?
                    .ok_or_else(|| anyhow!("can't find tx_hash from mem pool"))
            })
            .collect::<Result<_>>()?;
        let deposits = mem_block.deposits().to_vec();
        let withdrawals = mem_block
            .withdrawals()
            .iter()
            .map(|withdrawal_hash| {
                db.get_mem_pool_withdrawal(withdrawal_hash)?.ok_or_else(|| {
                    anyhow!(
                        "can't find withdrawal_hash from mem pool {}",
                        hex::encode(withdrawal_hash.as_slice())
                    )
                })
            })
            .collect::<Result<_>>()?;
        let state_checkpoint_list = mem_block.state_checkpoints().to_vec();
        let txs_prev_state_checkpoint = mem_block
            .txs_prev_state_checkpoint()
            .ok_or_else(|| anyhow!("Mem block has no txs prev state checkpoint"))?;
        let prev_merkle_state = mem_block.prev_merkle_state().clone();
        let parent_block = db
            .get_block(&self.current_tip().0)?
            .ok_or_else(|| anyhow!("can't found tip block"))?;

        let block_info = mem_block.block_info();
        let param = BlockParam {
            number: block_info.number().unpack(),
            block_producer_id: block_info.block_producer_id().unpack(),
            timestamp: block_info.timestamp().unpack(),
            txs,
            deposits,
            withdrawals,
            state_checkpoint_list,
            parent_block,
            txs_prev_state_checkpoint,
            prev_merkle_state,
            post_merkle_state,
            kv_state,
            kv_state_proof,
        };
        let finalized_custodians = mem_block.finalized_custodians().cloned();

        log::debug!(
            "output mem block, txs: {} tx withdrawals: {} state_checkpoints: {}",
            mem_block.txs().len(),
            mem_block.withdrawals().len(),
            mem_block.state_checkpoints().len(),
        );

        Ok((finalized_custodians, param))
    }

    fn package_mem_block(
        &self,
        output_param: &OutputParam,
    ) -> Result<(MemBlock, AccountMerkleState)> {
        let db = self.store.begin_transaction();
        let retry_count = output_param.retry_count;
        if retry_count == 0 {
            let mem_block = self.mem_block.clone();
            let mem_db_state = self.fetch_state_db(&db)?;

            return Ok((mem_block, mem_db_state.state_tree()?.merkle_state()?));
        }
        log::info!("[mem-pool] package mem block, retry count {}", retry_count);

        let mem_block = &self.mem_block;
        let state_db = self.fetch_state_db_with_mode(&db, MemBlockDBMode::Package)?;

        let (withdrawal_hashes, deposits, tx_hashes) = {
            let total =
                mem_block.withdrawals().len() + mem_block.deposits().len() + mem_block.txs().len();
            // Drop base on retry count
            let mut remain = total / (output_param.retry_count + 1);
            if 0 == remain {
                // Package at least one
                remain = 1;
            }

            let withdrawal_hashes = mem_block.withdrawals().iter().take(remain);
            remain = remain.saturating_sub(withdrawal_hashes.len());

            let deposits = mem_block.deposits().iter().take(remain);
            remain = remain.saturating_sub(deposits.len());

            let tx_hashes = mem_block.txs().iter().take(remain);

            (withdrawal_hashes, deposits, tx_hashes)
        };

        let mut repackage_block = MemBlock::new(
            mem_block.block_info().to_owned(),
            mem_block.prev_merkle_state().to_owned(),
        );

        assert!(repackage_block.state_checkpoints().is_empty());
        assert!(repackage_block.withdrawals().is_empty());
        assert!(repackage_block.finalized_custodians().is_none());
        assert!(repackage_block.deposits().is_empty());
        assert!(repackage_block.txs().is_empty());

        let mut state =
            state_db.state_tree_with_merkle_state(mem_block.prev_merkle_state().to_owned())?;

        // NOTE: Must have at least one tx to have correct post block state
        if withdrawal_hashes.len() == mem_block.withdrawals().len()
            && deposits.len() == mem_block.deposits().len()
            && tx_hashes.len() > 0
        {
            // Simply reuse mem block withdrawals and depoist result
            assert!(mem_block.state_checkpoints().len() >= withdrawal_hashes.len());
            for (hash, checkpoint) in withdrawal_hashes.zip(mem_block.state_checkpoints().iter()) {
                repackage_block.push_withdrawal(*hash, *checkpoint);
            }
            if let Some(finalized_custodians) = mem_block.finalized_custodians() {
                repackage_block.set_finalized_custodians(finalized_custodians.to_owned());
            }

            let deposit_cells = mem_block.deposits().to_vec();
            let prev_state_checkpoint = mem_block
                .txs_prev_state_checkpoint()
                .ok_or_else(|| anyhow!("repackage mem block but no prev state checkpoint"))?;
            repackage_block.push_deposits(deposit_cells, prev_state_checkpoint);

            repackage_block.append_touched_keys(mem_block.touched_keys().clone().into_iter());
        } else {
            assert_eq!(tx_hashes.len(), 0, "must drop txs first");
            log::info!(
                "[mem-pool] repackage withdrawals {} and deposits {}",
                withdrawal_hashes.len(),
                deposits.len()
            );

            state.tracker_mut().enable();

            // Repackage withdrawals
            let to_withdaral = |hash: &H256| -> Result<_> {
                db.get_mem_pool_withdrawal(hash)?
                    .ok_or_else(|| anyhow!("repackage {:?} withdrawal not found", hash))
            };
            let withdrawals: Vec<_> = withdrawal_hashes.map(to_withdaral).collect::<Result<_>>()?;

            for withdrawal in withdrawals.iter() {
                state.apply_withdrawal_request(
                    self.generator.rollup_context(),
                    mem_block.block_producer_id(),
                    withdrawal,
                )?;

                repackage_block.push_withdrawal(
                    withdrawal.hash().into(),
                    state.calculate_state_checkpoint()?,
                );
            }
            if let Some(finalized_custodians) = mem_block.finalized_custodians() {
                repackage_block.set_finalized_custodians(finalized_custodians.to_owned());
            }

            // Repackage deposits
            let deposit_cells: Vec<_> = deposits.cloned().collect();
            let deposits: Vec<_> = deposit_cells.iter().map(|i| i.request.clone()).collect();
            state.apply_deposit_requests(self.generator.rollup_context(), &deposits)?;
            let prev_state_checkpoint = state.calculate_state_checkpoint()?;
            repackage_block.push_deposits(deposit_cells, prev_state_checkpoint);

            let touched_keys = state.tracker_mut().touched_keys().expect("touched keys");
            repackage_block.append_touched_keys(touched_keys.borrow().iter().cloned());
        }

        // Repackage txs
        let mut post_tx_merkle_state = None;
        let tx_len = tx_hashes.len();
        for (idx, tx_hash) in tx_hashes.into_iter().enumerate() {
            let tx_receipt = db
                .get_mem_pool_transaction_receipt(tx_hash)?
                .ok_or_else(|| anyhow!("tx {:?} receipt not found", tx_hash))?;

            repackage_block.push_tx(*tx_hash, &tx_receipt);

            if idx + 1 == tx_len {
                post_tx_merkle_state = Some(tx_receipt.post_state())
            }
        }

        let post_merkle_state = match post_tx_merkle_state {
            Some(state) => state,
            None => state.merkle_state()?,
        };

        Ok((repackage_block, post_merkle_state))
    }

    /// Reset
    /// this method reset the current state of the mem pool
    /// discarded txs & withdrawals will be reinject to pool
    fn reset(&mut self, old_tip: Option<H256>, new_tip: Option<H256>) -> Result<()> {
        let mut reinject_txs = Default::default();
        let mut reinject_withdrawals = Default::default();
        // read block from db
        let new_tip = match new_tip {
            Some(block_hash) => block_hash,
            None => self.store.get_tip_block_hash()?,
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
                reinject_withdrawals = discarded_withdrawals;
            }
        }

        // estimate next l2block timestamp
        let estimated_timestamp = smol::block_on(self.provider.estimate_next_blocktime())?;
        // reset mem block state
        let merkle_state = new_tip_block.raw().post_account();
        let db = self.store.begin_transaction();
        self.reset_mem_block_state_db(&db, merkle_state)?;
        let mem_block_content = self.mem_block.reset(&new_tip_block, estimated_timestamp);
        db.update_mem_pool_block_info(self.mem_block.block_info())?;
        let reverted_block_root: H256 = {
            let smt = db.reverted_block_smt()?;
            smt.root().to_owned()
        };
        if let Some(ref mut offchain_validator) = self.offchain_validator {
            let timestamp = self.mem_block.block_info().timestamp().unpack();
            offchain_validator.reset(&new_tip_block, timestamp, reverted_block_root);
        }

        // set tip
        self.inner
            .set_current_tip((new_tip, new_tip_block.raw().number().unpack()));

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
        self.remove_unexecutables(&db)?;

        log::info!("[mem-pool] reset reinject txs: {} mem-block txs: {} reinject withdrawals: {} mem-block withdrawals: {}", reinject_txs.len(), mem_block_txs.len(), reinject_withdrawals.len(), mem_block_withdrawals.len());
        // re-inject withdrawals
        let withdrawals_iter = reinject_withdrawals
            .into_iter()
            .chain(mem_block_withdrawals);
        // re-inject txs
        let txs_iter = reinject_txs.into_iter().chain(mem_block_txs);
        self.prepare_next_mem_block(&db, withdrawals_iter, txs_iter)?;
        db.commit()?;

        Ok(())
    }

    /// Discard unexecutables from pending.
    fn remove_unexecutables(&mut self, db: &StoreTransaction) -> Result<()> {
        let state_db = self.fetch_state_db(db)?;
        let state = state_db.state_tree()?;
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

    fn reset_mem_block_state_db(
        &self,
        db: &StoreTransaction,
        merkle_state: AccountMerkleState,
    ) -> Result<()> {
        db.clear_mem_block_state()?;
        db.set_mem_block_account_count(merkle_state.count().unpack())?;
        db.set_mem_block_account_smt_root(merkle_state.merkle_root().unpack())?;
        Ok(())
    }

    /// Prepare for next mem block
    fn prepare_next_mem_block<
        WithdrawalIter: Iterator<Item = WithdrawalRequest>,
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
        // query deposit cells
        let task = self.provider.collect_deposit_cells();
        // Handle state before txs
        // withdrawal
        self.finalize_withdrawals(db, withdrawals.collect())?;
        // deposits
        let deposit_cells = {
            let cells = smol::block_on(task)?;
            crate::deposit::sanitize_deposit_cells(self.generator.rollup_context(), cells)
        };
        self.finalize_deposits(db, deposit_cells)?;
        // re-inject txs
        for tx in txs {
            if let Err(err) = self.push_transaction_with_db(db, tx.clone()) {
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

    fn finalize_deposits(
        &mut self,
        db: &StoreTransaction,
        deposit_cells: Vec<DepositInfo>,
    ) -> Result<()> {
        let state_db = self.fetch_state_db(db)?;
        let mut state = state_db.state_tree()?;
        // update deposits
        let deposits: Vec<_> = deposit_cells.iter().map(|c| c.request.clone()).collect();
        state.tracker_mut().enable();
        state.apply_deposit_requests(self.generator.rollup_context(), &deposits)?;
        // calculate state after withdrawals & deposits
        let prev_state_checkpoint = state.calculate_state_checkpoint()?;
        self.mem_block
            .push_deposits(deposit_cells, prev_state_checkpoint);
        state.submit_tree_to_mem_block()?;
        if let Some(ref mut offchain_validator) = self.offchain_validator {
            offchain_validator.set_prev_txs_checkpoint(prev_state_checkpoint);
        }
        let touched_keys = state.tracker_mut().touched_keys().expect("touched keys");
        self.mem_block
            .append_touched_keys(touched_keys.borrow().iter().cloned());
        Ok(())
    }

    /// Execute withdrawal & update local state
    fn finalize_withdrawals(
        &mut self,
        db: &StoreTransaction,
        mut withdrawals: Vec<WithdrawalRequest>,
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
                .last_finalized_block_number(self.current_tip().1);
            let task = self.provider.query_available_custodians(
                withdrawals.clone(),
                last_finalized_block_number,
                self.generator.rollup_context().to_owned(),
            );
            smol::block_on(task)?
        };

        let available_custodians = AvailableCustodians::from(&finalized_custodians);
        let asset_scripts: HashMap<H256, Script> = {
            let sudt_value = available_custodians.sudt.values();
            sudt_value.map(|(_, script)| (script.hash().into(), script.to_owned()))
        }
        .collect();
        let state_db = self.fetch_state_db(db)?;
        let mut state = state_db.state_tree()?;
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
                .check_withdrawal_request_signature(&state, &withdrawal)
            {
                log::info!("[mem-pool] withdrawal signature error: {:?}", err);
                unused_withdrawals.push(withdrawal_hash);
                continue;
            }
            let asset_script = asset_scripts
                .get(&withdrawal.raw().sudt_script_hash().unpack())
                .cloned();
            if let Err(err) =
                self.generator
                    .verify_withdrawal_request(&state, &withdrawal, asset_script)
            {
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

            if let Some(ref mut offchain_validator) = self.offchain_validator {
                match offchain_validator.verify_withdrawal_request(
                    db,
                    &state_db,
                    withdrawal.clone(),
                ) {
                    Ok(cycles) => log::debug!("[mem-pool] offchain withdrawal cycles {:?}", cycles),
                    Err(err) => match err.downcast_ref::<RollBackSavePointError>() {
                        Some(err) => bail!("{}", err),
                        None => {
                            log::info!(
                                "[mem-pool] withdrawal contextual verification failed : {}",
                                err
                            );
                            unused_withdrawals.push(withdrawal_hash);
                            continue;
                        }
                    },
                }
            }

            // update the state
            match state.apply_withdrawal_request(
                self.generator.rollup_context(),
                self.mem_block.block_producer_id(),
                &withdrawal,
            ) {
                Ok(_) => {
                    self.mem_block.push_withdrawal(
                        withdrawal.hash().into(),
                        state.calculate_state_checkpoint()?,
                    );
                }
                Err(err) => {
                    log::info!("[mem-pool] withdrawal execution failed : {}", err);
                    unused_withdrawals.push(withdrawal_hash);
                }
            }
        }
        state.submit_tree_to_mem_block()?;
        let touched_keys = state.tracker_mut().touched_keys().expect("touched keys");
        self.mem_block
            .append_touched_keys(touched_keys.borrow().iter().cloned());
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
    fn finalize_tx(&mut self, db: &StoreTransaction, tx: L2Transaction) -> Result<TxReceipt> {
        let state_db = self.fetch_state_db(db)?;
        let mut state = state_db.state_tree()?;
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
        )?;

        if let Some(ref mut offchain_validator) = self.offchain_validator {
            let maybe_cycles =
                offchain_validator.verify_transaction(db, &state_db, tx.clone(), &run_result);

            if 0 == run_result.exit_code {
                let cycles = maybe_cycles?;
                log::debug!("[mem-pool] offchain verify tx cycles {:?}", cycles);
            }
        }

        if run_result.exit_code != 0 {
            let tx_hash: H256 = tx.hash().into();
            let block_number = self.mem_block.block_info().number().unpack();

            let receipt = ErrorTxReceipt {
                tx_hash,
                block_number,
                return_data: run_result.return_data,
                last_log: run_result.logs.last().cloned(),
            };
            if let Some(ref mut error_tx_handler) = self.error_tx_handler {
                error_tx_handler.handle_error_receipt(receipt).detach();
            }

            return Err(TransactionError::InvalidExitCode(run_result.exit_code).into());
        }

        // apply run result
        state.apply_run_result(&run_result)?;
        state.submit_tree_to_mem_block()?;

        // generate tx receipt
        let merkle_state = state.merkle_state()?;
        let tx_receipt =
            TxReceipt::build_receipt(tx.witness_hash().into(), run_result, merkle_state);

        Ok(tx_receipt)
    }
}
