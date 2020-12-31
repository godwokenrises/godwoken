use crate::next_block_context::NextBlockContext;
use anyhow::{anyhow, Result};
use gw_common::{
    blake2b::new_blake2b,
    smt::{Store, H256 as SMTH256},
    state::State,
    H256,
};
use gw_generator::{
    error::{LockAlgorithmError, ValidateError},
    traits::{CodeStore, StateExt},
    Generator, RunResult, TxReceipt,
};
use gw_store::OverlayStore;
use gw_types::{
    packed::{BlockInfo, DepositionRequest, L2Block, L2Transaction, WithdrawalRequest},
    prelude::*,
};
use std::{cmp::min, collections::HashSet};

/// MAX mem pool txs
const MAX_IN_POOL_TXS: usize = 6000;
/// MAX mem pool withdrawal requests
const MAX_IN_POOL_WITHDRAWAL: usize = 3000;
/// MAX packaged txs in a l2block
const MAX_PACKAGED_TXS: usize = 100;
/// MAX packaged withdrawal in a l2block
const MAX_PACKAGED_WITHDRAWAL: usize = 10;
const MAX_DATA_BYTES_LIMIT: usize = 25_000;

/// TODO remove txs from pool if a new block already contains txs
pub struct TxPool<S> {
    state: OverlayStore<S>,
    generator: Generator,
    queue: Vec<(L2Transaction, TxReceipt)>,
    withdrawal_queue: Vec<WithdrawalRequest>,
    next_block_info: BlockInfo,
    next_prev_account_state: MerkleState,
    rollup_type_script_hash: H256,
}

impl<S: Store<SMTH256>> TxPool<S> {
    pub fn create(
        state: OverlayStore<S>,
        generator: Generator,
        tip: &L2Block,
        nb_ctx: NextBlockContext,
    ) -> Result<Self> {
        let queue = Vec::with_capacity(MAX_PACKAGED_TXS);
        let withdrawal_queue = Vec::with_capacity(MAX_PACKAGED_WITHDRAWAL);
        let next_prev_account_state = get_account_state(&state)?;
        let next_block_info = gen_next_block_info(tip, nb_ctx)?;
        let rollup_type_script_hash = generator.rollup_type_script_hash.into();
        Ok(TxPool {
            state,
            generator,
            queue,
            withdrawal_queue,
            next_block_info,
            next_prev_account_state,
            rollup_type_script_hash,
        })
    }
}

impl<S: Store<SMTH256>> TxPool<S> {
    /// Push a layer2 tx into pool
    pub fn push(&mut self, tx: L2Transaction) -> Result<RunResult> {
        if self.queue.len() >= MAX_IN_POOL_TXS {
            return Err(anyhow!(
                "Too many txs in the pool! MAX_IN_POOL_TXS: {}",
                MAX_IN_POOL_TXS
            ));
        }
        // 1. execute tx
        let run_result = self.execute(tx.clone())?;
        // 2. update state
        self.state.apply_run_result(&run_result)?;
        // 3. push tx to pool
        let tx_witness_hash = tx.witness_hash().into();
        let compacted_post_account_root = self.state.calculate_compacted_account_root()?;
        let receipt = TxReceipt {
            tx_witness_hash,
            compacted_post_account_root,
            read_data_hashes: run_result.read_data.iter().map(|(hash, _)| *hash).collect(),
        };
        self.queue.push((tx, receipt));
        Ok(run_result)
    }

    /// Execute tx without push it into pool
    pub fn execute(&self, tx: L2Transaction) -> Result<RunResult> {
        // 1. verify tx signature
        self.verify_tx(&tx)?;
        // 2. execute contract
        let raw_tx = tx.raw();
        let run_result = self
            .generator
            .execute(&self.state, &self.next_block_info, &raw_tx)?;
        let write_data_bytes: usize = run_result.write_data.values().map(|data| data.len()).sum();
        if write_data_bytes > MAX_DATA_BYTES_LIMIT {
            return Err(anyhow!(
                "tx write data exceeded the limitation. write data bytes: {} max data bytes: {}",
                write_data_bytes,
                MAX_DATA_BYTES_LIMIT
            ));
        }
        let read_data_bytes: usize = run_result.read_data.values().sum();
        if read_data_bytes > MAX_DATA_BYTES_LIMIT {
            return Err(anyhow!(
                "tx read data exceeded the limitation. read data bytes: {} max data bytes: {}",
                read_data_bytes,
                MAX_DATA_BYTES_LIMIT
            ));
        }
        Ok(run_result)
    }

    /// Push a withdrawal request into pool
    pub fn push_withdrawal_request(&mut self, withdrawal_request: WithdrawalRequest) -> Result<()> {
        if self.withdrawal_queue.len() >= MAX_IN_POOL_WITHDRAWAL {
            return Err(anyhow!(
                "Too many withdrawal in the pool! MAX_IN_POOL_WITHDRAWAL: {}",
                MAX_IN_POOL_WITHDRAWAL
            ));
        }
        self.verify_withdrawal_request(&withdrawal_request)?;
        self.withdrawal_queue.push(withdrawal_request);
        Ok(())
    }

    pub fn verify_withdrawal_request(&self, withdrawal_request: &WithdrawalRequest) -> Result<()> {
        self.generator
            .verify_withdrawal_request(&self.state, withdrawal_request)
            .map_err(Into::into)
    }

    fn verify_tx(&self, tx: &L2Transaction) -> Result<()> {
        let raw_tx = tx.raw();
        let sender_id: u32 = raw_tx.from_id().unpack();

        // verify nonce
        let account_nonce: u32 = self.state.get_nonce(sender_id).expect("get nonce");
        let nonce: u32 = raw_tx.nonce().unpack();
        if nonce != account_nonce {
            return Err(anyhow!(
                "invalid nonce, expected {}, actual {}",
                account_nonce,
                nonce
            ));
        }

        // verify signature
        let script_hash = self.state.get_script_hash(sender_id)?;
        if script_hash.is_zero() {
            return Err(anyhow!(
                "can not find script hash for account id: {}",
                sender_id
            ));
        }
        let script = self.state.get_script(&script_hash).expect("get script");
        let lock_code_hash: [u8; 32] = script.code_hash().unpack();

        let mut hasher = new_blake2b();
        hasher.update(self.rollup_type_script_hash.as_slice());
        hasher.update(&raw_tx.as_slice());
        let mut message = [0u8; 32];
        hasher.finalize(&mut message);

        let lock_algo = self
            .generator
            .account_lock_manage()
            .get_lock_algorithm(&lock_code_hash.into())
            .ok_or(ValidateError::UnknownAccountLockScript)?;
        let valid_signature =
            lock_algo.verify_signature(script.args().unpack(), tx.signature(), message.into())?;
        if !valid_signature {
            return Err(LockAlgorithmError::InvalidSignature.into());
        }
        Ok(())
    }

    /// Package
    /// this method return a tx pool package contains txs and withdrawal requests,
    /// and remove these from the pool
    pub fn package(&mut self, deposition_requests: &[DepositionRequest]) -> Result<TxPoolPackage> {
        let txs_limit = min(MAX_PACKAGED_TXS, self.queue.len());
        let tx_receipts = self.queue.iter().take(txs_limit).cloned().collect();
        // reset overlay, we need to record deposition / withdrawal touched keys to generate proof for state
        self.state.overlay_store_mut().clear_touched_keys();
        // fetch withdrawal request and rerun verifier, drop invalid requests
        let withdrawal_limit = min(MAX_PACKAGED_WITHDRAWAL, self.withdrawal_queue.len());
        let withdrawal_requests: Vec<_> = self
            .withdrawal_queue
            .iter()
            .take(withdrawal_limit)
            .cloned()
            .collect();
        // TODO make sure the remain capacity is enough to pay custodian cell
        // apply withdrawal request to the state
        self.state.apply_withdrawal_requests(&withdrawal_requests)?;
        // apply deposition request to the state
        self.state.apply_deposition_requests(&deposition_requests)?;
        let post_account_state = get_account_state(&self.state)?;
        let touched_keys = self
            .state
            .overlay_store_mut()
            .touched_keys()
            .into_iter()
            .map(|k| (*k).into())
            .collect();
        let pkg = TxPoolPackage {
            touched_keys,
            tx_receipts,
            prev_account_state: self.next_prev_account_state.clone(),
            post_account_state,
            withdrawal_requests,
        };
        Ok(pkg)
    }

    /// Update tip and state
    /// this method reset tip and tx_pool states
    pub fn update_tip(
        &mut self,
        tip: &L2Block,
        state: OverlayStore<S>,
        nb_ctx: NextBlockContext,
    ) -> Result<()> {
        self.state = state;
        self.update_tip_without_status(tip, nb_ctx)?;
        Ok(())
    }

    /// Update tip
    /// this method reset tip and generate a new checkpoint for current state
    ///
    /// Notice this fucntion may cause inconsistency between tip and status
    pub fn update_tip_without_status(
        &mut self,
        tip: &L2Block,
        nb_ctx: NextBlockContext,
    ) -> Result<()> {
        self.next_block_info = gen_next_block_info(tip, nb_ctx)?;
        self.next_prev_account_state = get_account_state(&self.state)?;
        // re-verify txs
        let queue: Vec<_> = self.queue.drain(..).collect();
        for (tx, _receipt) in queue {
            if self.push(tx.clone()).is_err() {
                let tx_hash: ckb_types::H256 = tx.hash().into();
                eprintln!("TxPool: drop tx {}", tx_hash);
            }
        }
        let withdrawal_queue: Vec<_> = self.withdrawal_queue.drain(..).collect();
        for request in withdrawal_queue {
            if self.push_withdrawal_request(request.clone()).is_err() {
                eprintln!("TxPool: drop withdrawal {:?}", request);
            }
        }
        Ok(())
    }
}

fn get_account_state<S: State>(state: &S) -> Result<MerkleState> {
    let root = state.calculate_root()?;
    let count = state.get_account_count()?;
    Ok(MerkleState { root, count })
}

fn gen_next_block_info(tip: &L2Block, nb_ctx: NextBlockContext) -> Result<BlockInfo> {
    let parent_number: u64 = tip.raw().number().unpack();
    let block_info = BlockInfo::new_builder()
        .aggregator_id(nb_ctx.aggregator_id.pack())
        .number((parent_number + 1).pack())
        .timestamp(nb_ctx.timestamp.pack())
        .build();
    Ok(block_info)
}

#[derive(Clone, Debug)]
pub struct MerkleState {
    pub root: H256,
    pub count: u32,
}

/// TxPoolPackage
/// a layer2 block can be generated from a package
pub struct TxPoolPackage {
    /// tx receipts
    pub tx_receipts: Vec<(L2Transaction, TxReceipt)>,
    /// txs touched keys, both reads and writes
    pub touched_keys: HashSet<H256>,
    /// state of last block
    pub prev_account_state: MerkleState,
    /// state after handling depositin requests
    pub post_account_state: MerkleState,
    /// withdrawal requests
    pub withdrawal_requests: Vec<WithdrawalRequest>,
}
