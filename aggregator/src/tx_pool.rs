use crate::crypto::{verify_signature, Signature};
use anyhow::{anyhow, Result};
use gw_common::{
    blake2b::new_blake2b, merkle_utils::calculate_compacted_account_root, state::State,
};
use gw_generator::{state_ext::StateExt, Generator, GetContractCode};
use gw_types::{
    packed::{BlockInfo, L2Block, L2Transaction},
    prelude::*,
};

const MAX_PACKAGED_TXS: usize = 6000;

pub struct TxRecipt {
    tx: L2Transaction,
    // hash(account_root|account_count)
    compacted_post_account_root: [u8; 32],
}

pub struct TxPool<S, CS> {
    state: S,
    generator: Generator<CS>,
    queue: Vec<TxRecipt>,
    tip_block_info: BlockInfo,
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
                .execute(&self.state, &self.tip_block_info, &call_context)?;
        let account_root = self
            .state
            .calculate_root()
            .map_err(|err| anyhow!("calculate account root error: {:?}", err))?;
        let account_count = self
            .state
            .get_account_count()
            .map_err(|err| anyhow!("get account count error: {:?}", err))?;
        let compacted_post_account_root =
            calculate_compacted_account_root(&account_root, account_count);
        // 3. push tx to pool
        self.queue.push(TxRecipt {
            tx,
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
    pub fn package_txs(&mut self) -> Result<TxPackage> {
        let txs = self.queue.drain(..MAX_PACKAGED_TXS).collect();
        Ok(txs)
    }

    /// Update tip block
    /// this method reset tip and tx_pool states
    pub fn update_tip(&mut self, tip: &L2Block, state: S) -> Result<TxPackage> {
        // TODO catch abandoned txs and recompute them.
        unimplemented!()
    }
}

/// TxPackage
/// a layer2 block can be generated from a package
pub type TxPackage = Vec<TxRecipt>;
