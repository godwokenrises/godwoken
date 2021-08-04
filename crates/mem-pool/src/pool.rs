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
use gw_generator::{traits::StateExt, Generator};
use gw_rpc_client::RPCClient;
use gw_store::{
    chain_view::ChainView,
    state_db::{CheckPoint, StateDBMode, StateDBTransaction, StateTree, SubState, WriteContext},
    transaction::StoreTransaction,
    Store,
};
use gw_types::{
    offchain::{DepositInfo, RunResult},
    packed::{
        AccountMerkleState, BlockInfo, L2Block, L2Transaction, RawL2Transaction, Script, TxReceipt,
        WithdrawalRequest,
    },
    prelude::{Entity, Pack, Unpack},
};
use rand::Rng;
use std::{
    cmp::{max, min},
    collections::{HashMap, HashSet},
    sync::Arc,
};

use crate::{mem_block::MemBlock, withdrawal::AvailableCustodians};

/// MAX deposits in the mem block
const MAX_MEM_BLOCK_DEPOSITS: usize = 50;
/// MAX withdrawals in the mem block
const MAX_MEM_BLOCK_WITHDRAWALS: usize = 50;
/// MAX withdrawals in the mem block
const MAX_MEM_BLOCK_TXS: usize = 500;
/// MAX mem pool txs
const MAX_IN_POOL_TXS: usize = 6000;
/// MAX mem pool withdrawals
const MAX_IN_POOL_WITHDRAWAL: usize = 3000;
/// MAX tx size 50 KB
const MAX_TX_SIZE: usize = 50_000;
/// MAX withdrawal size 50 KB
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
    /// RPC client
    rpc_client: RPCClient,
}

impl MemPool {
    pub fn create(store: Store, generator: Arc<Generator>, rpc_client: RPCClient) -> Result<Self> {
        let pending = Default::default();
        let all_txs = Default::default();
        let all_withdrawals = Default::default();

        let tip_block = store.get_tip_block()?;
        let tip = (tip_block.hash().into(), tip_block.raw().number().unpack());

        let mut mem_pool = MemPool {
            store,
            current_tip: tip,
            generator,
            pending,
            all_txs,
            all_withdrawals,
            rpc_client,
            mem_block: Default::default(),
        };

        // set tip
        mem_pool.reset(None, Some(tip.0))?;
        Ok(mem_pool)
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

        // reject if mem block is full
        // TODO: we can use the pool as a buffer
        if self.mem_block.txs().len() >= MAX_MEM_BLOCK_TXS {
            return Err(anyhow!(
                "Mem block is full, MAX_MEM_BLOCK_TXS: {}",
                MAX_MEM_BLOCK_TXS
            ));
        }

        // remove withdrawal request with lower or equal tx nonce
        let account_id: u32 = tx.raw().from_id().unpack();

        // Check replace-by-fee
        // TODO

        // instantly run tx in background & update local state
        let db = self.store.begin_transaction();
        let tx_receipt = self.finalize_tx(&db, tx.clone())?;
        db.commit()?;
        // save tx receipt in mem pool
        self.mem_block.push_tx(tx.hash().into(), tx_receipt);

        // Add to pool
        // TODO check nonce conflict
        self.all_txs.insert(tx_hash, tx.clone());
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
        self.reset(Some(self.current_tip.0), Some(new_tip))?;
        // finalize next mem block
        self.finalize_next_mem_block()?;
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

    /// Re-package mem block
    /// This function reset mem block status & re-package current pool into mem block
    pub fn repackage(&mut self) -> Result<()> {
        // reset pool state
        self.reset(Some(self.current_tip.0), Some(self.current_tip.0))?;
        // finalize next mem block
        self.finalize_next_mem_block()?;
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

        // reset mem block state
        let merkle_state = new_tip_block.raw().post_account();
        self.reset_mem_block_state_db(merkle_state)?;
        self.mem_block.reset(&new_tip_block);

        // set tip
        self.current_tip = (new_tip, new_tip_block.raw().number().unpack());

        // re-inject withdrawals
        for withdrawal in reinject_withdrawals {
            if self.push_withdrawal_request(withdrawal.clone()).is_err() {
                log::info!("MemPool: drop withdrawal {:?}", withdrawal);
            }
        }

        if !reinject_txs.is_empty() {
            // re-inject txs
            for tx in reinject_txs {
                if self.push_transaction(tx.clone()).is_err() {
                    log::info!("MemPool: drop tx {:?}", tx.hash());
                }
            }
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

    /// finalize next mem block
    fn finalize_next_mem_block(&mut self) -> Result<()> {
        if self.mem_block.txs().is_empty() {
            // query deposit cells
            let task = {
                let rpc_client = self.rpc_client.clone();
                smol::spawn(async move { rpc_client.query_deposit_cells().await })
            };
            // finalize withdrawals
            let db = self.store.begin_transaction();
            self.finalize_withdrawals(&db)?;
            // finalize deposits
            let deposit_cells = {
                let cells = smol::block_on(task)?;
                crate::deposit::sanitize_deposit_cells(self.generator.rollup_context(), cells)
            };
            self.finalize_deposits(&db, deposit_cells)?;
        }
        Ok(())
    }

    fn finalize_deposits(
        &mut self,
        db: &StoreTransaction,
        deposit_cells: Vec<DepositInfo>,
    ) -> Result<()> {
        let state_db = self.fetch_state_db(db)?;
        let mut state = state_db.account_state_tree()?;
        // update deposits
        let deposits: Vec<_> = deposit_cells.iter().map(|c| c.request.clone()).collect();
        state.apply_deposit_requests(self.generator.rollup_context(), &deposits)?;
        // calculate state after withdrawals & deposits
        let prev_state_checkpoint = state.calculate_state_checkpoint()?;
        self.mem_block
            .push_deposits(deposit_cells, prev_state_checkpoint);
        state.submit_tree_to_mem_block()?;
        Ok(())
    }

    /// Execute withdrawal & update local state
    fn finalize_withdrawals(&mut self, db: &StoreTransaction) -> Result<()> {
        // check mem block state
        assert!(self.mem_block.withdrawals().is_empty());
        assert!(self.mem_block.state_checkpoints().is_empty());
        assert!(self.mem_block.deposits().is_empty());
        assert!(self.mem_block.tx_receipts().is_empty());

        let mut withdrawal_requests = Vec::new();

        // find withdrawals from pending
        {
            for entry in self.pending().values() {
                if !entry.withdrawals.is_empty()
                    && withdrawal_requests.len() < MAX_MEM_BLOCK_WITHDRAWALS
                {
                    withdrawal_requests.push(entry.withdrawals.first().unwrap().clone());
                }
            }
        };

        let max_withdrawal_capacity = std::u128::MAX;
        let available_custodians =
            AvailableCustodians::build(db, &self.rpc_client, &withdrawal_requests)?;
        let asset_scripts: HashMap<H256, Script> = {
            let sudt_value = available_custodians.sudt.values();
            sudt_value.map(|(_, script)| (script.hash().into(), script.to_owned()))
        }
        .collect();
        let state_db = self.fetch_state_db(db)?;
        let mut state = state_db.account_state_tree()?;
        // verify the withdrawals
        let mut unused_withdrawal_requests = Vec::with_capacity(withdrawal_requests.len());
        let mut total_withdrawal_capacity: u128 = 0;
        let mut withdrawal_verifier = crate::withdrawal::Generator::new(
            self.generator.rollup_context(),
            available_custodians,
        );
        for request in withdrawal_requests {
            // check withdrawal request
            if let Err(err) = self
                .generator
                .check_withdrawal_request_signature(&state, &request)
            {
                log::info!("[mem-pool] withdrawal signature error: {:?}", err);
                unused_withdrawal_requests.push(request);
                continue;
            }
            let asset_script = asset_scripts
                .get(&request.raw().sudt_script_hash().unpack())
                .cloned();
            if let Err(err) =
                self.generator
                    .verify_withdrawal_request(&state, &request, asset_script)
            {
                log::info!("[mem-pool] withdrawal verification error: {:?}", err);
                unused_withdrawal_requests.push(request);
                continue;
            }
            let capacity: u64 = request.raw().capacity().unpack();
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
                unused_withdrawal_requests.push(request);
                continue;
            }
            total_withdrawal_capacity = new_total_withdrwal_capacity;

            if let Err(err) = withdrawal_verifier.include_and_verify(&request, &L2Block::default())
            {
                log::info!(
                    "[mem-pool] withdrawal contextual verification failed : {}",
                    err
                );
                unused_withdrawal_requests.push(request);
                continue;
            }

            // update the state
            match state.apply_withdrawal_request(
                self.generator.rollup_context(),
                self.mem_block.block_producer_id(),
                &request,
            ) {
                Ok(_) => {
                    self.mem_block.push_withdrawal(
                        request.hash().into(),
                        state.calculate_state_checkpoint()?,
                    );
                }
                Err(err) => {
                    log::info!("[mem-pool] withdrawal execution failed : {}", err);
                    unused_withdrawal_requests.push(request);
                }
            }
        }
        state.submit_tree_to_mem_block()?;
        log::info!(
            "[mem-pool] finalize withdrawals: {} staled withdrawals: {}",
            self.mem_block.withdrawals().len(),
            unused_withdrawal_requests.len()
        );
        Ok(())
    }

    /// Execute tx & update local state
    fn finalize_tx(&mut self, db: &StoreTransaction, tx: L2Transaction) -> Result<TxReceipt> {
        let state_db = self.fetch_state_db(&db)?;
        let mut state = state_db.account_state_tree()?;
        let tip_block_hash = db.get_tip_block_hash()?;
        let chain_view = ChainView::new(&db, tip_block_hash);

        let block_info = self.mem_block.block_info();

        // execute tx
        let raw_tx = tx.raw();
        let run_result =
            self.generator
                .execute_transaction(&chain_view, &state, &block_info, &raw_tx)?;

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
