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
use gw_challenge::offchain::{OffChainCancelChallengeValidator, OffChainValidatorContext};
use gw_common::{
    builtins::CKB_SUDT_ACCOUNT_ID,
    state::{to_short_address, State},
    H256,
};
use gw_generator::{traits::StateExt, Generator};
use gw_store::{
    chain_view::ChainView,
    state_db::{CheckPoint, StateDBMode, StateDBTransaction, SubState, WriteContext},
    transaction::StoreTransaction,
    Store,
};
use gw_types::{
    offchain::{BlockParam, DepositInfo, RunResult},
    packed::{
        AccountMerkleState, BlockInfo, L2Block, L2Transaction, RawL2Transaction, Script, TxReceipt,
        WithdrawalRequest,
    },
    prelude::{Entity, Pack, Unpack},
};
use std::{
    cmp::{max, min},
    collections::{HashMap, HashSet},
    sync::Arc,
};

use crate::{
    constants::{
        MAX_IN_POOL_TXS, MAX_IN_POOL_WITHDRAWAL, MAX_MEM_BLOCK_TXS, MAX_MEM_BLOCK_WITHDRAWALS,
        MAX_TX_SIZE, MAX_WITHDRAWAL_SIZE,
    },
    mem_block::MemBlock,
    traits::MemPoolProvider,
    types::EntryList,
};

/// MemPool
pub struct MemPool {
    /// store
    store: Store,
    /// current tip
    current_tip: (H256, u64),
    /// generator instance
    generator: Arc<Generator>,
    /// pending queue, contains executable contents(can be pacakged into block)
    pending: HashMap<u32, EntryList>,
    /// all transactions in the pool
    all_txs: HashMap<H256, L2Transaction>,
    /// all withdrawals in the pool
    all_withdrawals: HashMap<H256, WithdrawalRequest>,
    /// memory block
    mem_block: MemBlock,
    /// Mem pool provider
    provider: Box<dyn MemPoolProvider + Send>,
    /// Offchain cancel challenge validator
    offchain_validator: Option<OffChainCancelChallengeValidator>,
}

impl MemPool {
    pub fn create(
        store: Store,
        generator: Arc<Generator>,
        provider: Box<dyn MemPoolProvider + Send>,
        offchain_validator_context: Option<OffChainValidatorContext>,
    ) -> Result<Self> {
        let pending = Default::default();
        let all_txs = Default::default();
        let all_withdrawals = Default::default();

        let tip_block = store.get_tip_block()?;
        let tip = (tip_block.hash().into(), tip_block.raw().number().unpack());

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
                reverted_block_root,
            )
        });

        let mut mem_pool = MemPool {
            store,
            current_tip: tip,
            generator,
            pending,
            all_txs,
            all_withdrawals,
            mem_block,
            provider,
            offchain_validator,
        };

        // set tip
        mem_pool.reset(None, Some(tip.0))?;
        Ok(mem_pool)
    }

    pub fn mem_block(&self) -> &MemBlock {
        &self.mem_block
    }

    pub fn all_txs(&self) -> &HashMap<H256, L2Transaction> {
        &self.all_txs
    }

    pub fn all_withdrawals(&self) -> &HashMap<H256, WithdrawalRequest> {
        &self.all_withdrawals
    }

    pub fn set_provider(&mut self, provider: Box<dyn MemPoolProvider + Send>) {
        self.provider = provider;
    }

    pub fn fetch_state_db<'a>(&self, db: &'a StoreTransaction) -> Result<StateDBTransaction<'a>> {
        let offset = (self.mem_block.withdrawals().len() + self.mem_block.txs().len()) as u32;
        StateDBTransaction::from_checkpoint(
            db,
            CheckPoint::new(self.current_tip.1, SubState::MemBlock(offset)),
            StateDBMode::Write(WriteContext::default()),
        )
        .map_err(|err| anyhow!("err: {}", err))
    }

    /// Push a layer2 tx into pool
    pub fn push_transaction(&mut self, tx: L2Transaction) -> Result<()> {
        // check duplication
        let tx_hash: H256 = tx.raw().hash().into();
        if self.mem_block.tx_receipts().contains_key(&tx_hash) {
            return Err(anyhow!("duplicated tx"));
        }

        // remove under price tx if pool is full
        if self.all_txs.len() >= MAX_IN_POOL_TXS {
            //TODO
            return Err(anyhow!(
                "Too many txs in the pool! MAX_IN_POOL_TXS: {}",
                MAX_IN_POOL_TXS
            ));
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
        self.verify_tx(&tx)?;

        // instantly run tx in background & update local state
        let db = self.store.begin_transaction();
        let tx_receipt = self.finalize_tx(&db, tx.clone())?;
        db.commit()?;

        // save tx receipt in mem pool
        self.mem_block.push_tx(tx_hash, tx_receipt);

        // Add to pool
        let account_id: u32 = tx.raw().from_id().unpack();
        self.all_txs.insert(tx_hash, tx.clone());
        let entry_list = self.pending.entry(account_id).or_default();
        entry_list.txs.push(tx);

        Ok(())
    }

    /// verify tx
    fn verify_tx(&self, tx: &L2Transaction) -> Result<()> {
        // check tx size
        if tx.as_slice().len() > MAX_TX_SIZE {
            return Err(anyhow!("tx over size"));
        }

        let db = self.store.begin_transaction();
        let state_db = self.fetch_state_db(&db)?;
        let state = state_db.state_tree()?;
        // verify signature
        self.generator.check_transaction_signature(&state, &tx)?;
        self.generator.verify_transaction(&state, &tx)?;

        Ok(())
    }

    /// Execute tx without push it into pool
    pub fn execute_transaction(
        &self,
        tx: L2Transaction,
        block_info: &BlockInfo,
    ) -> Result<RunResult> {
        let db = self.store.begin_transaction();
        let state_db = self.fetch_state_db(&db)?;
        let state = state_db.state_tree()?;
        let tip_block_hash = self.store.get_tip_block_hash()?;
        let chain_view = ChainView::new(&db, tip_block_hash);
        // verify tx signature
        self.generator.check_transaction_signature(&state, &tx)?;
        // tx basic verification
        self.generator.verify_transaction(&state, &tx)?;
        // execute tx
        let raw_tx = tx.raw();
        let run_result =
            self.generator
                .execute_transaction(&chain_view, &state, &block_info, &raw_tx)?;
        Ok(run_result)
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
        let run_result =
            self.generator
                .execute_transaction(&chain_view, &state, &block_info, &raw_tx)?;
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
        if self.all_withdrawals.contains_key(&withdrawal_hash) {
            return Err(anyhow!("duplicated withdrawal"));
        }

        // basic verification
        self.verify_withdrawal_request(&withdrawal)?;

        // remove under price tx if pool is full
        if self.all_withdrawals.len() >= MAX_IN_POOL_WITHDRAWAL {
            //TODO
            return Err(anyhow!(
                "Too many withdrawals in the pool! MAX_IN_POOL_WITHDRAWALS: {}",
                MAX_IN_POOL_WITHDRAWAL
            ));
        }
        // Check replace-by-fee
        // TODO

        // Add to pool
        // TODO check nonce conflict
        self.all_withdrawals
            .insert(withdrawal_hash, withdrawal.clone());
        let db = self.store.begin_transaction();
        let state_db = self.fetch_state_db(&db)?;
        let state = state_db.state_tree()?;
        let account_script_hash: H256 = withdrawal.raw().account_script_hash().unpack();
        let account_id = state
            .get_account_id_by_script_hash(&account_script_hash)?
            .expect("get account_id");
        let entry_list = self.pending.entry(account_id).or_default();
        entry_list.withdrawals.push(withdrawal);
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
        self.reset(Some(self.current_tip.0), Some(new_tip))?;
        Ok(())
    }

    /// Clear mem block state and recollect deposits
    pub fn reset_mem_block(&mut self) -> Result<()> {
        log::info!("[mem-pool] reset mem block");
        // reset pool state
        self.reset(Some(self.current_tip.0), Some(self.current_tip.0))?;
        Ok(())
    }

    /// output mem block
    pub fn output_mem_block(&self) -> Result<BlockParam> {
        let db = self.store.begin_transaction();
        let state_db = StateDBTransaction::from_checkpoint(
            &db,
            CheckPoint::new(self.current_tip.1, SubState::Block),
            StateDBMode::ReadOnly,
        )?;
        // generate kv state & merkle proof from tip state
        let state = state_db.state_tree()?;

        let kv_state: Vec<(H256, H256)> = self
            .mem_block
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

        let txs = self
            .mem_block
            .txs()
            .iter()
            .map(|tx_hash| {
                self.all_txs
                    .get(tx_hash)
                    .map(ToOwned::to_owned)
                    .ok_or_else(|| anyhow!("can't find tx_hash from mem pool"))
            })
            .collect::<Result<_>>()?;
        let deposits = self.mem_block.deposits().to_vec();
        let withdrawals = self
            .mem_block
            .withdrawals()
            .iter()
            .map(|withdrawal_hash| {
                self.all_withdrawals
                    .get(withdrawal_hash)
                    .map(ToOwned::to_owned)
                    .ok_or_else(|| {
                        anyhow!(
                            "can't find withdrawal_hash from mem pool {}",
                            hex::encode(withdrawal_hash.as_slice())
                        )
                    })
            })
            .collect::<Result<_>>()?;
        let state_checkpoint_list = self.mem_block.state_checkpoints().to_vec();
        let txs_prev_state_checkpoint = self
            .mem_block
            .txs_prev_state_checkpoint()
            .ok_or_else(|| anyhow!("Mem block has no txs prev state checkpoint"))?;
        let prev_merkle_state = self.mem_block.prev_merkle_state().clone();
        let post_merkle_state = {
            let mem_db_state = self.fetch_state_db(&db)?;
            let mem_state = mem_db_state.state_tree()?;
            mem_state.get_merkle_state()
        };
        let parent_block = db
            .get_block(&self.current_tip.0)?
            .ok_or_else(|| anyhow!("can't found tip block"))?;

        let block_info = self.mem_block.block_info();
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

        log::debug!(
            "output mem block, txs: {} tx receipts: {} state_checkpoints: {}",
            self.mem_block.txs().len(),
            self.mem_block.tx_receipts().len(),
            self.mem_block.state_checkpoints().len(),
        );

        Ok(param)
    }

    /// Reset
    /// this method reset the current state of the mem pool
    /// discarded txs & withdrawals will be reinject to pool
    fn reset(&mut self, old_tip: Option<H256>, new_tip: Option<H256>) -> Result<()> {
        let mut reinject_txs: Vec<L2Transaction> = Default::default();
        let mut reinject_withdrawals: Vec<WithdrawalRequest> = Default::default();
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
                let mut discarded_txs: HashSet<L2Transaction> = Default::default();
                let mut included_txs: HashSet<L2Transaction> = Default::default();
                let mut discarded_withdrawals: HashSet<WithdrawalRequest> = Default::default();
                let mut included_withdrawals: HashSet<WithdrawalRequest> = Default::default();
                while rem.raw().number().unpack() > add.raw().number().unpack() {
                    discarded_txs.extend(rem.transactions().into_iter());
                    discarded_withdrawals.extend(rem.withdrawals().into_iter());
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
                    discarded_txs.extend(rem.transactions().into_iter());
                    discarded_withdrawals.extend(rem.withdrawals().into_iter());
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
                reinject_txs = discarded_txs
                    .difference(&included_txs)
                    .into_iter()
                    .cloned()
                    .collect();
                reinject_withdrawals = discarded_withdrawals
                    .difference(&included_withdrawals)
                    .into_iter()
                    .cloned()
                    .collect();
            }
        }

        // reset mem block state
        let merkle_state = new_tip_block.raw().post_account();
        self.reset_mem_block_state_db(merkle_state)?;
        // estimate next l2block timestamp
        let estimated_timestamp = smol::block_on(self.provider.estimate_next_blocktime())?;
        let mem_block_content = self.mem_block.reset(&new_tip_block, estimated_timestamp);
        let reverted_block_root: H256 = {
            let db = self.store.begin_transaction();
            let smt = db.reverted_block_smt()?;
            smt.root().to_owned()
        };
        if let Some(ref mut offchain_validator) = self.offchain_validator {
            offchain_validator.reset(&new_tip_block, reverted_block_root);
        }

        // set tip
        self.current_tip = (new_tip, new_tip_block.raw().number().unpack());

        // mem block withdrawals
        let mem_block_withdrawals: Vec<_> = mem_block_content
            .withdrawals
            .into_iter()
            .filter_map(|withdrawal_hash| self.all_withdrawals.get(&withdrawal_hash))
            .cloned()
            .collect();
        // re-inject withdrawals
        let withdrawals_iter = reinject_withdrawals
            .into_iter()
            .chain(mem_block_withdrawals.into_iter());

        // Process txs
        let mem_block_txs: Vec<_> = mem_block_content
            .txs
            .into_iter()
            .filter_map(|tx_hash| self.all_txs.get(&tx_hash))
            .cloned()
            .collect();

        // remove from pending
        self.remove_unexecutables()?;

        // re-inject txs
        let txs_iter = reinject_txs.into_iter().chain(mem_block_txs.into_iter());
        self.prepare_next_mem_block(withdrawals_iter, txs_iter)?;

        Ok(())
    }

    /// Discard unexecutables from pending.
    fn remove_unexecutables(&mut self) -> Result<()> {
        let db = self.store.begin_transaction();
        let state_db = self.fetch_state_db(&db)?;
        let state = state_db.state_tree()?;
        let mut remove_list = Vec::default();
        // iter pending accounts and demote any non-executable objects
        for (&account_id, list) in &mut self.pending {
            let nonce = state.get_nonce(account_id)?;

            // drop txs if tx.nonce lower than nonce
            let deprecated_txs = list.remove_lower_nonce_txs(nonce);
            for tx in deprecated_txs {
                let tx_hash = tx.hash().into();
                self.all_txs.remove(&tx_hash);
            }
            // Drop all withdrawals that are have no enough balance
            let script_hash = state.get_script_hash(account_id)?;
            let capacity =
                state.get_sudt_balance(CKB_SUDT_ACCOUNT_ID, to_short_address(&script_hash))?;
            let deprecated_withdrawals = list.remove_lower_nonce_withdrawals(nonce, capacity);
            for withdrawal in deprecated_withdrawals {
                let withdrawal_hash: H256 = withdrawal.hash().into();
                self.all_withdrawals.remove(&withdrawal_hash);
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

    fn reset_mem_block_state_db(&self, merkle_state: AccountMerkleState) -> Result<()> {
        let db = self.store.begin_transaction();
        db.clear_mem_block_state()?;
        db.set_mem_block_account_count(merkle_state.count().unpack())?;
        db.set_mem_block_account_smt_root(merkle_state.merkle_root().unpack())?;
        db.commit()?;
        Ok(())
    }

    /// Prepare for next mem block
    fn prepare_next_mem_block<
        WithdrawalIter: Iterator<Item = WithdrawalRequest>,
        TxIter: Iterator<Item = L2Transaction>,
    >(
        &mut self,
        withdrawals: WithdrawalIter,
        txs: TxIter,
    ) -> Result<()> {
        // query deposit cells
        let task = self.provider.collect_deposit_cells();
        // Handle state before txs
        let db = self.store.begin_transaction();
        // withdrawal
        self.finalize_withdrawals(&db, withdrawals.collect())?;
        // deposits
        let deposit_cells = {
            let cells = smol::block_on(task)?;
            crate::deposit::sanitize_deposit_cells(self.generator.rollup_context(), cells)
        };
        self.finalize_deposits(&db, deposit_cells)?;
        // save mem block state
        db.commit()?;
        // re-inject txs
        for tx in txs {
            if let Err(err) = self.push_transaction(tx.clone()) {
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
        assert!(self.mem_block.tx_receipts().is_empty());

        // find withdrawals from pending
        if withdrawals.is_empty() {
            for entry in self.pending().values() {
                if !entry.withdrawals.is_empty() && withdrawals.len() < MAX_MEM_BLOCK_WITHDRAWALS {
                    withdrawals.push(entry.withdrawals.first().unwrap().clone());
                }
            }
        }

        let max_withdrawal_capacity = std::u128::MAX;
        let available_custodians = {
            // query withdrawals from ckb-indexer
            let last_finalized_block_number = self
                .generator
                .rollup_context()
                .last_finalized_block_number(self.current_tip.1);
            let task = self.provider.query_available_custodians(
                withdrawals.clone(),
                last_finalized_block_number,
                self.generator.rollup_context().to_owned(),
            );
            smol::block_on(task)?
        };
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
                if let Err(err) =
                    offchain_validator.verify_withdrawal_request(db, &state_db, withdrawal.clone())
                {
                    log::info!(
                        "[mem-pool] withdrawal contextual verification failed : {}",
                        err
                    );
                    unused_withdrawals.push(withdrawal_hash);
                    continue;
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
        let state_db = self.fetch_state_db(&db)?;
        let mut state = state_db.state_tree()?;
        let tip_block_hash = db.get_tip_block_hash()?;
        let chain_view = ChainView::new(&db, tip_block_hash);

        let block_info = self.mem_block.block_info();

        // execute tx
        let raw_tx = tx.raw();
        let run_result =
            self.generator
                .execute_transaction(&chain_view, &state, &block_info, &raw_tx)?;

        if let Some(ref mut offchain_validator) = self.offchain_validator {
            offchain_validator.verify_transaction(db, &state_db, tx.clone(), &run_result)?;
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
