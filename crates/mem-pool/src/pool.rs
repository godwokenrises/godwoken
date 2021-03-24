#![allow(clippy::mutable_key_type)]
#![allow(clippy::unnecessary_unwrap)]
//! MemPool
//!
//! MemPool does not guarantee that tx & withdraw is fully valid,
//! the block producer need to verify them again before put them into a block.
//!
//! The main function of MemPool is to sort txs & withdrawls in memory,
//! so we can easily discard invalid objects and return sorted objects to block producer.
//!
//! The design of Godwoken MemPool is highly inspired by the Geth TxPool.
//! We maintain a pending list which contains executable txs & withdrawals (executable means can be packaged into the next block),
//! we also maintain a queue list which contains non-executable txs & withdrawals (these objects may become executable in the future).

use anyhow::{anyhow, Result};
use gw_common::{builtins::CKB_SUDT_ACCOUNT_ID, state::State, H256};
use gw_generator::{Generator, RunResult};
use gw_store::{
    chain_view::ChainView,
    state_db::{StateDBTransaction, StateDBVersion},
    Store,
};
use gw_types::{
    packed::{BlockInfo, L2Transaction, WithdrawalRequest},
    prelude::{Entity, Unpack},
};
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

pub struct MemPool {
    /// current state
    state_db: StateDBTransaction,
    /// store
    db: Store,
    /// current tip
    /// TODO remove me after version based storage
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
    pub fn create(db: Store, generator: Arc<Generator>) -> Result<Self> {
        let pending = Default::default();
        let all_txs = Default::default();
        let all_withdrawals = Default::default();

        let tip = db.get_tip_block_hash()?;

        let state_db = db.state_at(StateDBVersion::from_block_hash(tip))?;

        let mut mem_pool = MemPool {
            db,
            state_db,
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

    pub fn state_db(&self) -> &StateDBTransaction {
        &self.state_db
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
        // Check replace-by-fee
        // TODO

        // Add to pool
        // TODO check nonce conflict
        self.all_txs.insert(tx_hash, tx.clone());
        let account_id: u32 = tx.raw().from_id().unpack();
        let entry_list = self.pending.entry(account_id).or_default();
        entry_list.txs.push(tx);
        Ok(())
    }

    /// Basic verification for tx
    fn basic_verify_tx(&self, tx: &L2Transaction) -> Result<()> {
        // check tx size
        if tx.as_slice().len() > MAX_TX_SIZE {
            return Err(anyhow!("tx over size"));
        }

        // TODO
        // we should introduce queue manchanism and only remove tx when tx.nonce is lower
        // reject tx if nonce is not equals account.nonce
        let state = self.state_db.account_state_tree()?;
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
        let state = self.state_db.account_state_tree()?;
        let tip_block_hash = self.db.get_tip_block_hash()?;
        let chain_view = ChainView::new(self.db.begin_transaction(), tip_block_hash);
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
        let state = self.state_db.account_state_tree()?;
        self.all_withdrawals
            .insert(withdrawal_hash, withdrawal.clone());
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
        let state = self.state_db.account_state_tree()?;
        // verify withdrawal signature
        self.generator
            .check_withdrawal_request_signature(&state, withdrawal_request)?;
        // withdrawal basic verification
        self.generator
            .verify_withdrawal_request(&state, withdrawal_request)
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
        // try promote executables
        self.promote_executables(self.pending.iter())?;
        // try demote unexecutables, this function also discards objects that already in the chain
        self.demote_unexecutables()?;
        Ok(())
    }

    /// Move executables into pending.
    /// TODO
    #[allow(clippy::unnecessary_wraps)]
    fn promote_executables<I: Iterator>(&self, _accounts: I) -> Result<()> {
        Ok(())
    }

    /// Discard unexecutables from pending.
    fn demote_unexecutables(&mut self) -> Result<()> {
        let state = self.state_db.account_state_tree()?;
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
            let capacity = state.get_sudt_balance(CKB_SUDT_ACCOUNT_ID, account_id)?;
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
            None => self.db.get_tip_block_hash()?,
        };
        let new_tip_block = self.db.get_block(&new_tip)?.expect("new tip block");

        if old_tip.is_some() && old_tip != Some(new_tip_block.raw().parent_block_hash().unpack()) {
            let old_tip = old_tip.unwrap();
            let old_tip_block = self.db.get_block(&old_tip)?.expect("old tip block");

            let new_number: u64 = new_tip_block.raw().number().unpack();
            let old_number: u64 = old_tip_block.raw().number().unpack();
            let depth = max(new_number, old_number) - min(new_number, old_number);
            if depth > 64 {
                eprintln!("skipping deep transaction reorg: depth {}", depth);
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
                        .db
                        .get_block(&rem.raw().parent_block_hash().unpack())?
                        .expect("get block");
                }
                while add.raw().number().unpack() > rem.raw().number().unpack() {
                    included_txs.extend(add.transactions().into_iter());
                    included_withdrawals.extend(rem.withdrawals().into_iter());
                    add = self
                        .db
                        .get_block(&add.raw().parent_block_hash().unpack())?
                        .expect("get block");
                }
                while rem.hash() != add.hash() {
                    discarded_txs.extend(rem.transactions().into_iter());
                    discarded_withdrawals.extend(rem.withdrawals().into_iter());
                    rem = self
                        .db
                        .get_block(&rem.raw().parent_block_hash().unpack())?
                        .expect("get block");
                    included_txs.extend(add.transactions().into_iter());
                    included_withdrawals.extend(add.withdrawals().into_iter());
                    add = self
                        .db
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
        self.state_db = self
            .db
            .state_at(StateDBVersion::from_block_hash(tip_block_hash))?;

        // re-inject txs
        for tx in reinject_txs {
            if self.push_transaction(tx.clone()).is_err() {
                eprintln!("MemPool: drop tx {:?}", tx.hash());
            }
        }
        // re-inject withdrawals
        for withdrawal in reinject_withdrawals {
            if self.push_withdrawal_request(withdrawal.clone()).is_err() {
                eprintln!("MemPool: drop withdrawal {:?}", withdrawal);
            }
        }
        Ok(())
    }
}
