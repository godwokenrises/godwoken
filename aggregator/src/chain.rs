use crate::collector::Collector;
use crate::config::ChainConfig;
use crate::consensus::traits::Consensus;
use crate::deposition::fetch_deposition_requests;
use crate::jsonrpc_types::collector::QueryParam;
use crate::state_impl::{OverlayState, StateImpl, WrapStore};
use crate::tx_pool::TxPool;
use anyhow::{anyhow, Result};
use ckb_types::{
    bytes::Bytes,
    packed::{RawTransaction, Transaction, WitnessArgs, WitnessArgsReader},
    prelude::Unpack,
};
use gw_common::{merkle_utils::calculate_merkle_root, sparse_merkle_tree};
use gw_generator::{
    generator::{DepositionRequest, StateTransitionArgs},
    syscalls::GetContractCode,
    Generator,
};
use gw_types::{
    packed::{AccountMerkleState, L2Block, L2BlockReader, RawL2Block, SubmitTransactions},
    prelude::{
        Builder as GWBuilder, Entity as GWEntity, Pack as GWPack, Reader as GWReader,
        Unpack as GWUnpack,
    },
};
use parking_lot::Mutex;
use std::sync::Arc;
use std::time::SystemTime;

pub struct HeaderInfo {
    pub number: u64,
    pub block_hash: [u8; 32],
}

/// concrete type aliases
pub type StateStore = sparse_merkle_tree::default_store::DefaultStore<sparse_merkle_tree::H256>;
pub type TxPoolImpl<CodeStore> = TxPool<OverlayState<WrapStore<StateStore>>, CodeStore>;

pub struct Chain<Collector, CodeStore, Consensus> {
    config: ChainConfig,
    state: StateImpl<StateStore>,
    collector: Collector,
    last_synced: HeaderInfo,
    tip: L2Block,
    generator: Generator<CodeStore>,
    tx_pool: Arc<Mutex<TxPoolImpl<CodeStore>>>,
    consensus: Consensus,
}

impl<Collec: Collector, CodeStore: GetContractCode, Consen: Consensus>
    Chain<Collec, CodeStore, Consen>
{
    pub fn new(
        config: ChainConfig,
        state: StateImpl<StateStore>,
        consensus: Consen,
        tip: L2Block,
        last_synced: HeaderInfo,
        collector: Collec,
        generator: Generator<CodeStore>,
        tx_pool: Arc<Mutex<TxPoolImpl<CodeStore>>>,
    ) -> Self {
        Chain {
            config,
            state,
            collector,
            last_synced,
            tip,
            generator,
            tx_pool,
            consensus,
        }
    }

    /// Sync chain from layer1
    pub fn sync(&mut self) -> Result<()> {
        // TODO handle rollback
        if self
            .collector
            .get_header(&self.last_synced.block_hash)?
            .is_none()
        {
            panic!("layer1 chain has forked!")
        }
        // query state update tx from collector
        let param = QueryParam {
            type_: Some(self.config.rollup_type_script.clone().into()),
            from_block: Some(self.last_synced.number.into()),
            ..Default::default()
        };
        let txs = self.collector.query_transactions(param)?;
        // apply tx to state
        for tx_info in txs {
            let header = self
                .collector
                .get_header(&tx_info.block_hash)?
                .expect("should not panic unless the chain is forking");
            let block_number: u64 = header.raw().number().unpack();
            assert!(
                block_number > self.last_synced.number,
                "must greater than last synced number"
            );

            // parse layer2 block
            let rollup_id = self.config.rollup_type_script.calc_script_hash().unpack();
            let l2block = parse_l2block(&tx_info.transaction, &rollup_id)?;

            let tip_number: u64 = self.tip.raw().number().unpack();
            assert!(
                l2block.raw().number().unpack() == tip_number + 1,
                "new l2block number must be the successor of the tip"
            );

            // process l2block
            self.process_block(l2block.clone(), &tx_info.transaction.raw(), &rollup_id)?;

            // update chain
            self.last_synced = HeaderInfo {
                number: header.raw().number().unpack(),
                block_hash: header.calc_header_hash().unpack(),
            };
            self.tip = l2block;
        }
        // update tx pool state
        let overlay_state = self.state.new_overlay()?;
        let nb_ctx = self.consensus.next_block_context(&self.tip);
        self.tx_pool
            .lock()
            .update_tip(&self.tip, overlay_state, nb_ctx)?;
        Ok(())
    }

    /// Produce a new block
    ///
    /// This function should be called in the turn that the current aggregator to produce the next block,
    /// otherwise the produced block may invalided by the state-validator contract.
    pub fn produce_block(
        &mut self,
        deposition_requests: Vec<DepositionRequest>,
    ) -> Result<RawL2Block> {
        let signer = self
            .config
            .signer
            .as_ref()
            .ok_or(anyhow!("signer is not configured!"))?;
        // take txs from tx pool
        // produce block
        let pkg = self.tx_pool.lock().package_txs(&deposition_requests)?;
        let parent_number: u64 = self.tip.raw().number().unpack();
        let number = parent_number + 1;
        let aggregator_id: u32 = signer.account_id;
        let timestamp: u64 = unixtime()?;
        let submit_txs = {
            let tx_witness_root = calculate_merkle_root(
                pkg.tx_recipts
                    .iter()
                    .map(|tx_recipt| &tx_recipt.tx_witness_hash)
                    .cloned()
                    .collect(),
            )
            .map_err(|err| anyhow!("merkle root error: {:?}", err))?;
            let tx_count = pkg.tx_recipts.len() as u32;
            let compacted_post_root_list: Vec<_> = pkg
                .tx_recipts
                .iter()
                .map(|tx_recipt| &tx_recipt.compacted_post_account_root)
                .cloned()
                .collect();
            SubmitTransactions::new_builder()
                .tx_witness_root(tx_witness_root.pack())
                .tx_count(tx_count.pack())
                .compacted_post_root_list(compacted_post_root_list.pack())
                .build()
        };
        let prev_account = AccountMerkleState::new_builder()
            .merkle_root(pkg.prev_account_state.root.pack())
            .count(pkg.prev_account_state.count.pack())
            .build();
        let post_account = AccountMerkleState::new_builder()
            .merkle_root(pkg.post_account_state.root.pack())
            .count(pkg.post_account_state.count.pack())
            .build();
        let raw_block = RawL2Block::new_builder()
            .number(number.pack())
            .aggregator_id(aggregator_id.pack())
            .timestamp(timestamp.pack())
            .post_account(post_account)
            .prev_account(prev_account)
            .submit_transactions(Some(submit_txs).pack())
            .valid(1.into())
            .build();
        Ok(raw_block)
    }

    fn process_block(
        &mut self,
        l2block: L2Block,
        tx: &RawTransaction,
        rollup_id: &[u8; 32],
    ) -> Result<()> {
        let deposition_requests = fetch_deposition_requests(&self.collector, tx, rollup_id)?;
        let args = StateTransitionArgs {
            l2block,
            deposition_requests,
        };
        self.generator
            .apply_state_transition(&mut self.state, args)?;
        Ok(())
    }
}

fn unixtime() -> Result<u64> {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .map_err(Into::into)
}

fn parse_l2block(tx: &Transaction, rollup_id: &[u8; 32]) -> Result<L2Block> {
    // find rollup state cell from outputs
    let (i, _) = tx
        .raw()
        .outputs()
        .into_iter()
        .enumerate()
        .find(|(_i, output)| {
            output
                .type_()
                .to_opt()
                .map(|type_| type_.calc_script_hash().unpack())
                .as_ref()
                == Some(rollup_id)
        })
        .ok_or_else(|| anyhow!("no rollup cell found"))?;

    let witness: Bytes = tx
        .witnesses()
        .get(i)
        .ok_or_else(|| anyhow!("no witness"))?
        .unpack();
    let witness_args = match WitnessArgsReader::verify(&witness, false) {
        Ok(_) => WitnessArgs::new_unchecked(witness),
        Err(_) => {
            return Err(anyhow!("invalid witness"));
        }
    };
    let output_type: Bytes = witness_args
        .output_type()
        .to_opt()
        .ok_or_else(|| anyhow!("output_type field is none"))?
        .unpack();
    match L2BlockReader::verify(&output_type, false) {
        Ok(_) => Ok(L2Block::new_unchecked(output_type)),
        Err(_) => Err(anyhow!("invalid l2block")),
    }
}
