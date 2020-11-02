use crate::consensus::traits::NextBlockContext;
use crate::crypto::{verify_signature, Signature};
use anyhow::{anyhow, Result};
use gw_common::{
    blake2b::new_blake2b, merkle_utils::calculate_compacted_account_root, state::State,
};
use gw_generator::{generator::DepositionRequest, state_ext::StateExt, Generator, GetContractCode};
use gw_types::{
    packed::{BlockInfo, L2Block, L2Transaction},
    prelude::*,
};

const MAX_PACKAGED_TXS: usize = 6000;

pub struct TxRecipt {
    pub tx: L2Transaction,
    pub tx_witness_hash: [u8; 32],
    // hash(account_root|account_count)
    pub compacted_post_account_root: [u8; 32],
}

pub struct TxPool<S, CodeStore> {
    state: S,
    generator: Generator<CodeStore>,
    queue: Vec<TxRecipt>,
    next_block_info: BlockInfo,
    next_prev_account_state: MerkleState,
}

impl<S: State, CodeStore> TxPool<S, CodeStore> {
    pub fn create(
        state: S,
        generator: Generator<CodeStore>,
        tip: &L2Block,
        nb_ctx: NextBlockContext,
    ) -> Result<Self> {
        let queue = Vec::with_capacity(MAX_PACKAGED_TXS);
        let next_prev_account_state = get_account_state(&state)?;
        let next_block_info = gen_next_block_info(tip, nb_ctx)?;
        Ok(TxPool {
            state,
            generator,
            queue,
            next_block_info,
            next_prev_account_state,
        })
    }
}

impl<S: State, CS: GetContractCode> TxPool<S, CS> {
    /// Push a layer2 tx into pool
    pub fn push(&mut self, tx: L2Transaction) -> Result<()> {
        // 1. verify tx signature
        self.verify_tx(&tx)?;
        // 2. execute contract
        let call_context = tx.raw().to_call_context();
        let run_result =
            self.generator
                .execute(&self.state, &self.next_block_info, &call_context)?;
        // 3. push tx to pool
        let tx_witness_hash = {
            let mut witness_hash = [0u8; 32];
            let mut hasher = new_blake2b();
            hasher.update(tx.as_slice());
            hasher.finalize(&mut witness_hash);
            witness_hash
        };
        let compacted_post_account_root = {
            let account_root = self
                .state
                .calculate_root()
                .map_err(|err| anyhow!("calculate account root error: {:?}", err))?;
            let account_count = self
                .state
                .get_account_count()
                .map_err(|err| anyhow!("get account count error: {:?}", err))?;
            calculate_compacted_account_root(&account_root, account_count)
        };
        self.queue.push(TxRecipt {
            tx,
            tx_witness_hash,
            compacted_post_account_root,
        });

        // update state
        self.state
            .apply_run_result(&run_result)
            .map_err(|err| anyhow!("apply state error: {:?}", err))?;
        Ok(())
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
        let pubkey_hash = self
            .state
            .get_pubkey_hash(sender_id)
            .expect("get pubkey hash");
        let raw_tx_hash = {
            let mut buf = [0u8; 32];
            let mut hasher = new_blake2b();
            hasher.update(tx.raw().as_slice());
            hasher.finalize(&mut buf);
            buf
        };
        let sig = Signature(tx.signature().unpack());
        verify_signature(&sig, &raw_tx_hash, &pubkey_hash)?;
        Ok(())
    }

    /// Package txpool transactions
    /// this method return a tx package, and remove these txs from the pool
    pub fn package_txs(&mut self, deposition_requests: &[DepositionRequest]) -> Result<TxPackage> {
        let tx_recipts = self.queue.drain(..).collect();
        // handle deposition requests and calculate post state
        self.state
            .apply_deposition_requests(&deposition_requests)
            .map_err(|err| anyhow!("apply deposition requests: {:?}", err))?;
        let post_account_state = get_account_state(&self.state)?;
        let pkg = TxPackage {
            tx_recipts,
            prev_account_state: self.next_prev_account_state.clone(),
            post_account_state,
        };
        Ok(pkg)
    }

    /// Update tip and state
    /// this method reset tip and tx_pool states
    pub fn update_tip(&mut self, tip: &L2Block, state: S, nb_ctx: NextBlockContext) -> Result<()> {
        // TODO catch abandoned txs and recompute them.
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
        // TODO catch abandoned txs and recompute them.
        self.queue.clear();
        self.next_block_info = gen_next_block_info(tip, nb_ctx)?;
        self.next_prev_account_state = get_account_state(&self.state)?;
        Ok(())
    }
}

fn get_account_state<S: State>(state: &S) -> Result<MerkleState> {
    let root = state
        .calculate_root()
        .map_err(|err| anyhow!("calculate root: {:?}", err))?;
    let count = state
        .get_account_count()
        .map_err(|err| anyhow!("get account count: {:?}", err))?;
    Ok(MerkleState { root, count })
}

fn gen_next_block_info(tip: &L2Block, nb_ctx: NextBlockContext) -> Result<BlockInfo> {
    let parent_number: u64 = tip.raw().number().unpack();
    // TODO validate timestamp
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();
    let block_info = BlockInfo::new_builder()
        .aggregator_id(nb_ctx.aggregator_id.pack())
        .number((parent_number + 1).pack())
        .timestamp(timestamp.pack())
        .build();
    Ok(block_info)
}

#[derive(Clone, Debug)]
pub struct MerkleState {
    pub root: [u8; 32],
    pub count: u32,
}

/// TxPackage
/// a layer2 block can be generated from a package
pub struct TxPackage {
    /// tx recipts
    pub tx_recipts: Vec<TxRecipt>,
    /// state of last block
    pub prev_account_state: MerkleState,
    /// state after handling deposition requests
    pub post_account_state: MerkleState,
}
