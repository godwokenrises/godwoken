#![allow(clippy::mutable_key_type)]
#![allow(clippy::unnecessary_unwrap)]
//! MemPool
//!
//! MemPool supports two mode: Normal & InstantFinality
//!
//! Normal mode:
//! MemPool do not actually execute the transactions & withdrawals,
//! the execution is delayed to the producing of the next block.
//! In this mode, the block producer need to execute txs & withdrawals before produce new block.
//! The design of Godwoken MemPool is highly inspired by the Geth TxPool.
//! We maintain a pending list which contains executable txs & withdrawals (executable means can be packaged into the next block),
//! we also maintain a queue list which contains non-executable txs & withdrawals (these objects may become executable in the future).
//!
//! Instant mode:
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
use gw_generator::Generator;
use gw_store::{
    chain_view::ChainView,
    state_db::{CheckPoint, StateDBMode, StateDBTransaction, SubState},
    transaction::StoreTransaction,
    Store,
};
use gw_types::{
    offchain::RunResult,
    packed::{BlockInfo, L2Transaction, RawL2Transaction, WithdrawalRequest},
    prelude::{Entity, Unpack},
};
use rand::Rng;
use std::{
    cmp::{max, min},
    collections::{HashMap, HashSet},
    sync::Arc,
};

/// MAX mem pool txs
const MAX_IN_POOL_TXS: usize = 6000;
/// MAX mem pool withdrawal requests
const MAX_IN_POOL_WITHDRAWAL: usize = 3000;
/// MAX tx size
const MAX_TX_SIZE: usize = 50_000;
/// MAX withdrawal size
const MAX_WITHDRAWAL_SIZE: usize = 50_000;

#[derive(Default)]
pub struct EntryList {
    // txs sorted by nonce
    pub txs: Vec<L2Transaction>,
    // withdrawals sorted by nonce
    pub withdrawals: Vec<WithdrawalRequest>,
}

impl EntryList {
    fn is_empty(&self) -> bool {
        self.txs.is_empty() && self.withdrawals.is_empty()
    }

    // remove and return txs which tx.nonce is lower than nonce
    fn remove_lower_nonce_txs(&mut self, nonce: u32) -> Vec<L2Transaction> {
        let mut removed = Vec::default();
        while !self.txs.is_empty() {
            let tx_nonce: u32 = self.txs[0].raw().nonce().unpack();
            if tx_nonce >= nonce {
                break;
            }
            removed.push(self.txs.remove(0));
        }
        removed
    }

    // remove and return withdrawals which withdrawal.nonce is lower than nonce & have not enough balance
    fn remove_lower_nonce_balance_withdrawals(
        &mut self,
        nonce: u32,
        capacity: u128,
    ) -> Vec<WithdrawalRequest> {
        let mut removed = Vec::default();

        // remove lower nonce withdrawals
        while !self.withdrawals.is_empty() {
            let withdrawal_nonce: u32 = self.withdrawals[0].raw().nonce().unpack();
            if withdrawal_nonce >= nonce {
                break;
            }
            removed.push(self.withdrawals.remove(0));
        }

        // remove lower balance withdrawals
        if let Some(withdrawal) = self.withdrawals.get(0) {
            let withdrawal_capacity: u64 = withdrawal.raw().capacity().unpack();
            if (withdrawal_capacity as u128) > capacity {
                // TODO instead of remove all withdrawals, put them into future queue
                removed.extend_from_slice(&self.withdrawals);
                self.withdrawals.clear();
            }
        }

        removed
    }
}

/// Mem pool mode
#[derive(Debug, PartialEq, Eq)]
pub enum MemPoolMode {
    /// Normal mode, transactions are not executed until produce the next block
    Normal,
    /// InstantFinality mode, transactions are executed instantly
    InstantFinality,
}

/// MemPool
pub struct MemPool {
    /// mem pool mode
    mode: MemPoolMode,
    /// current state checkpoint
    state_checkpoint: CheckPoint,
    /// store
    store: Store,
    /// current tip
    current_tip: Option<H256>,
    /// generator instance
    generator: Arc<Generator>,
    /// pending queue, contains executable contents(can be pacakged into block)
    pending: HashMap<u32, EntryList>,
    /// all transactions in the pool
    all_txs: HashMap<H256, L2Transaction>,
    /// all withdrawals in the pool
    all_withdrawals: HashMap<H256, WithdrawalRequest>,
}

impl MemPool {
    pub fn create(mode: MemPoolMode, store: Store, generator: Arc<Generator>) -> Result<Self> {
        let pending = Default::default();
        let all_txs = Default::default();
        let all_withdrawals = Default::default();

        let tip = store.get_tip_block_hash()?;

        let state_checkpoint =
            CheckPoint::from_block_hash(&store.begin_transaction(), tip, SubState::Block)?;

        let mut mem_pool = MemPool {
            mode,
            store,
            state_checkpoint,
            current_tip: None,
            generator,
            pending,
            all_txs,
            all_withdrawals,
        };

        // set tip
        mem_pool.reset(None, Some(tip))?;
        Ok(mem_pool)
    }

    pub fn fetch_state_db<'a>(&self, db: &'a StoreTransaction) -> Result<StateDBTransaction<'a>> {
        StateDBTransaction::from_checkpoint(
            db,
            self.state_checkpoint.clone(),
            StateDBMode::ReadOnly,
        )
        .map_err(|err| anyhow!("err: {}", err))
    }

    /// Push a layer2 tx into pool
    pub fn push_transaction(&mut self, tx: L2Transaction) -> Result<()> {
        // check duplication
        let tx_hash: H256 = tx.raw().hash().into();
        if self.all_txs.contains_key(&tx_hash) {
            return Err(anyhow!("duplicated tx"));
        }

        // basic verification
        self.basic_verify_tx(&tx)?;

        // remove under price tx if pool is full
        if self.all_txs.len() >= MAX_IN_POOL_TXS {
            //TODO
            return Err(anyhow!(
                "Too many txs in the pool! MAX_IN_POOL_TXS: {}",
                MAX_IN_POOL_TXS
            ));
        }

        // remove withdrawal request with lower or equal tx nonce
        let account_id: u32 = tx.raw().from_id().unpack();
        let entry_list = self.pending.entry(account_id).or_default();
        let tx_nonce: u32 = tx.raw().nonce().unpack();
        entry_list.withdrawals.retain(|withdrawal| {
            let withdrawal_nonce: u32 = withdrawal.raw().nonce().unpack();
            withdrawal_nonce > tx_nonce
        });

        // Check replace-by-fee
        // TODO

        if self.mode == MemPoolMode::InstantFinality {
            // instantly run tx in background & update local state
        }

        // Add to pool
        // TODO check nonce conflict
        self.all_txs.insert(tx_hash, tx.clone());
        entry_list.txs.push(tx);

        Ok(())
    }

    /// Basic verification for tx
    fn basic_verify_tx(&self, tx: &L2Transaction) -> Result<()> {
        // check tx size
        if tx.as_slice().len() > MAX_TX_SIZE {
            return Err(anyhow!("tx over size"));
        }

        let db = self.store.begin_transaction();
        let state_db = self.fetch_state_db(&db)?;

        // TODO
        // we should introduce queue manchanism and only remove tx when tx.nonce is lower
        // reject tx if nonce is not equals account.nonce
        let state = state_db.account_state_tree()?;
        let account_id: u32 = tx.raw().from_id().unpack();
        let nonce = state.get_nonce(account_id)?;
        let tx_nonce: u32 = tx.raw().nonce().unpack();
        if nonce != tx_nonce {
            return Err(anyhow!(
                "tx's nonce is incorrect, expected: {} got: {}",
                nonce,
                tx_nonce,
            ));
        }

        // verify signature
        self.generator.check_transaction_signature(&state, &tx)?;

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
        let state = state_db.account_state_tree()?;
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
        block_number: u64,
    ) -> Result<RunResult> {
        let db = self.store.begin_transaction();
        let check_point = CheckPoint::new(block_number, SubState::Block);
        let state_db =
            StateDBTransaction::from_checkpoint(&db, check_point, StateDBMode::ReadOnly)?;
        let state = state_db.account_state_tree()?;
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

        if self.mode == MemPoolMode::InstantFinality {
            // instantly run withdrawal in background & update local state
        }

        // Add to pool
        // TODO check nonce conflict
        self.all_withdrawals
            .insert(withdrawal_hash, withdrawal.clone());
        let db = self.store.begin_transaction();
        let state_db = self.fetch_state_db(&db)?;
        let state = state_db.account_state_tree()?;
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
        let state = state_db.account_state_tree()?;
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
    pub fn pending(&self) -> &HashMap<u32, EntryList> {
        &self.pending
    }

    /// Notify new tip
    /// this method update current state of mem pool
    pub fn notify_new_tip(&mut self, new_tip: H256) -> Result<()> {
        // reset pool state
        self.reset(self.current_tip, Some(new_tip))?;
        self.current_tip = Some(new_tip);
        // under instant finality mode, all txs & withdrawal get executed or reject,
        // so we can skip promote / demote phase
        if self.mode == MemPoolMode::Normal {
            // try promote executables
            self.promote_executables(self.pending.iter())?;
            // try demote unexecutables, this function also discards objects that already in the chain
            self.demote_unexecutables()?;
        }
        Ok(())
    }

    /// FIXME: remove this hotfix function
    pub fn randomly_drop_withdrawals(&mut self) -> Result<usize> {
        const DEL_RATE: (u32, u32) = (1, 2);

        let mut rng = rand::thread_rng();
        let mut is_delete = || rng.gen_ratio(DEL_RATE.0, DEL_RATE.1);

        let mut deleted_count = 0;

        for list in self.pending.values_mut() {
            if !list.withdrawals.is_empty() && is_delete() {
                for w in &list.withdrawals {
                    self.all_withdrawals.remove(&w.hash().into());
                }
                deleted_count += list.withdrawals.len();
                list.withdrawals.clear();
            }
        }
        Ok(deleted_count)
    }

    /// Move executables into pending.
    /// TODO
    #[allow(clippy::unnecessary_wraps)]
    fn promote_executables<I: Iterator>(&self, _accounts: I) -> Result<()> {
        Ok(())
    }

    /// Discard unexecutables from pending.
    fn demote_unexecutables(&mut self) -> Result<()> {
        let db = self.store.begin_transaction();
        let state_db = self.fetch_state_db(&db)?;
        let state = state_db.account_state_tree()?;
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
            let deprecated_withdrawals =
                list.remove_lower_nonce_balance_withdrawals(nonce, capacity);
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

        // update current state
        let tip_block_hash = new_tip_block.hash().into();
        self.state_checkpoint = CheckPoint::from_block_hash(
            &self.store.begin_transaction(),
            tip_block_hash,
            SubState::Block,
        )?;

        // re-inject withdrawals
        for withdrawal in reinject_withdrawals {
            if self.push_withdrawal_request(withdrawal.clone()).is_err() {
                log::info!("MemPool: drop withdrawal {:?}", withdrawal);
            }
        }

        // re-inject txs
        for tx in reinject_txs {
            if self.push_transaction(tx.clone()).is_err() {
                log::info!("MemPool: drop tx {:?}", tx.hash());
            }
        }
        Ok(())
    }

    /// Execute withdrawal & update local state
    fn finalize_withdrawals(&self) -> Result<()> {
        Ok(())
    }

    /// Execute tx & update local state
    fn finalize_txs(&self, tx: L2Transaction, block_info: &BlockInfo) -> Result<RunResult> {
        let db = self.store.begin_transaction();
        let state_db = self.fetch_state_db(&db)?;
        let state = state_db.account_state_tree()?;
        let tip_block_hash = self.store.get_tip_block_hash()?;
        let chain_view = ChainView::new(&db, tip_block_hash);
        // execute tx
        let raw_tx = tx.raw();
        let run_result =
            self.generator
                .execute_transaction(&chain_view, &state, &block_info, &raw_tx)?;
        Ok(run_result)
    }
}
