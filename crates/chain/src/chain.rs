use crate::tx_pool::TxPool;
use crate::{next_block_context::NextBlockContext, tx_pool::TxPoolPackage};
use anyhow::{anyhow, Result};
use ckb_types::{
    bytes::Bytes,
    packed::{Script, Transaction, WitnessArgs, WitnessArgsReader},
    prelude::Unpack,
};
use gw_common::{
    h256_ext::H256Ext, merkle_utils::calculate_merkle_root, smt::Blake2bHasher, sparse_merkle_tree,
    state::State, H256,
};
use gw_config::ChainConfig;
use gw_generator::{generator::StateTransitionArgs, Error as GeneratorError, Generator};
use gw_store::{Store, WrapStore};
use gw_types::{
    packed::{
        AccountMerkleState, BlockMerkleState, CancelChallenge, DepositionRequest, GlobalState,
        HeaderInfo, L2Block, L2BlockReader, RawL2Block, StartChallenge, SubmitTransactions,
    },
    prelude::{
        Builder as GWBuilder, Entity as GWEntity, Pack as GWPack, PackVec as GWPackVec,
        Reader as GWReader, Unpack as GWUnpack,
    },
};
use parking_lot::Mutex;
use std::sync::Arc;
use std::time::SystemTime;

/// Rollup status
#[derive(Debug, Eq, PartialEq, Copy, Clone)]
pub enum Status {
    Running,
    Halting,
}

/// Produce block param
pub struct ProduceBlockParam {
    /// aggregator of this block
    pub aggregator_id: u32,
    /// tx pool package
    pub tx_pool_pkg: TxPoolPackage,
}

/// sync params
pub struct SyncParam {
    // contains transitions from tip to fork point
    pub reverts: Vec<L1Action>,
    /// contains transitions from fork point to new tips
    pub updates: Vec<L1Action>,
    pub next_block_context: NextBlockContext,
}

#[derive(Debug)]
pub enum L1ActionContext {
    SubmitTxs {
        /// deposition requests
        deposition_requests: Vec<DepositionRequest>,
    },
    Challenge {
        context: StartChallenge,
    },
    CancelChallenge {
        context: CancelChallenge,
    },
    Revert {
        context: StartChallenge,
    },
}

pub struct L1Action {
    /// transaction
    pub transaction: Transaction,
    /// transactions' header info
    pub header_info: HeaderInfo,
    pub context: L1ActionContext,
}

pub struct ProduceBlockResult {
    pub block: L2Block,
    pub global_state: GlobalState,
}

/// sync method returned events
pub enum SyncEvent {
    // success
    Success,
    // found a invalid block
    BadBlock(StartChallenge),
    // found a invalid challenge
    BadChallenge(CancelChallenge),
    // the rollup is in a challenge
    WaitChallenge,
}

/// concrete type aliases
pub type StateStore = sparse_merkle_tree::default_store::DefaultStore<sparse_merkle_tree::H256>;
pub type TxPoolImpl = TxPool<WrapStore<StateStore>>;

pub struct LocaLState {
    tip: L2Block,
    last_synced: HeaderInfo,
    current_bad_block: Option<StartChallenge>,
    status: Status,
}

impl LocaLState {
    fn new(tip: L2Block, last_synced: HeaderInfo, status: Status) -> Self {
        LocaLState {
            tip,
            last_synced,
            status,
            current_bad_block: None,
        }
    }

    /// return rollup status
    pub fn status(&self) -> Status {
        self.status
    }

    pub fn set_status(&mut self, status: Status) {
        self.status = status;
    }

    pub fn tip(&self) -> &L2Block {
        &self.tip
    }

    pub fn last_synced(&self) -> &HeaderInfo {
        &self.last_synced
    }

    fn set_current_bad_block(&mut self, context: Option<StartChallenge>) {
        self.current_bad_block = context;
    }
}

pub struct Chain {
    pub rollup_type_script_hash: [u8; 32],
    pub store: Store<StateStore>,
    pub local_state: LocaLState,
    pub generator: Generator,
    pub tx_pool: Arc<Mutex<TxPoolImpl>>,
}

impl Chain {
    pub fn create(
        config: ChainConfig,
        store: Store<StateStore>,
        generator: Generator,
        tx_pool: Arc<Mutex<TxPoolImpl>>,
    ) -> Result<Self> {
        let rollup_type_script: Script = config.rollup_type_script.clone().into();
        let rollup_type_script_hash = rollup_type_script.calc_script_hash().unpack();
        let tip = store
            .get_tip_block()?
            .ok_or(anyhow!("can't find tip from store"))?;
        let last_synced = store
            .get_block_synced_header_info(&tip.hash().into())?
            .ok_or(anyhow!("can't find HeaderInfo of tip"))?;
        let local_state = LocaLState::new(tip, last_synced, Status::Running);
        Ok(Chain {
            store,
            local_state,
            generator,
            tx_pool,
            rollup_type_script_hash,
        })
    }

    /// return local state
    pub fn local_state(&self) -> &LocaLState {
        &self.local_state
    }

    pub fn store(&self) -> &Store<StateStore> {
        &self.store
    }

    /// Sync chain from layer1
    pub fn sync(&mut self, param: SyncParam) -> Result<SyncEvent> {
        // TODO handle layer1 reorg
        if !param.reverts.is_empty() {
            panic!("layer1 chain has forked!")
        }
        // apply tx to state
        for action in param.updates {
            let L1Action {
                transaction,
                header_info,
                context,
            } = action;
            let block_number: u64 = header_info.number().unpack();
            assert!(
                block_number > {
                    let number: u64 = self.local_state.last_synced.number().unpack();
                    number
                },
                "must greater than last synced number"
            );

            match (self.local_state.status(), context) {
                (
                    Status::Running,
                    L1ActionContext::SubmitTxs {
                        deposition_requests,
                    },
                ) => {
                    // Submit transactions
                    // parse layer2 block
                    let l2block = parse_l2block(&transaction, &self.rollup_type_script_hash)?;
                    if let Some(start_challenge) =
                        self.process_block(l2block, header_info, deposition_requests)?
                    {
                        // stop syncing and return event
                        self.local_state
                            .set_current_bad_block(Some(start_challenge.clone()));
                        return Ok(SyncEvent::BadBlock(start_challenge));
                    }
                }
                (Status::Running, L1ActionContext::Challenge { context }) => {
                    // Challenge
                    self.local_state.set_status(Status::Halting);
                    if let Some(current_bad_block) = self.local_state.current_bad_block.as_ref() {
                        if current_bad_block.as_slice() == context.as_slice() {
                            // bad block is in challenge, just wait.
                            return Ok(SyncEvent::WaitChallenge);
                        }
                        let current_bad_block_number: u64 =
                            current_bad_block.block_number().unpack();
                        let challenge_block_number: u64 = context.block_number().unpack();
                        if challenge_block_number >= current_bad_block_number {
                            // Because of the block is later than a bad block we found we can't determine wether the block is bad.
                            // So we just wait for the end and send a new challenge.
                            return Ok(SyncEvent::WaitChallenge);
                        }

                        return Ok(SyncEvent::WaitChallenge);
                    }
                    // now, either we haven't found a bad block or the challenge is challenge a validate block
                    // in both cases the challenge is bad
                    let cancel_challenge = unimplemented!();
                    return Ok(SyncEvent::BadChallenge(cancel_challenge));
                }
                (Status::Halting, L1ActionContext::CancelChallenge { context: _ }) => {
                    self.local_state.set_status(Status::Running);
                }
                (Status::Halting, L1ActionContext::Revert { context }) => {
                    self.local_state.set_status(Status::Running);
                    assert_eq!(
                        self.local_state
                            .current_bad_block
                            .as_ref()
                            .map(|b| b.as_slice()),
                        Some(context.as_slice()),
                        "revert from the bad block"
                    );
                }
                (status, context) => {
                    panic!(
                        "unsupported syncing state: status {:?} context {:?}",
                        status, context
                    );
                }
            }
        }
        // update tx pool state
        let overlay_state = self.store.new_overlay()?;
        self.tx_pool.lock().update_tip(
            &self.local_state.tip,
            overlay_state,
            param.next_block_context,
        )?;
        Ok(SyncEvent::Success)
    }

    fn process_block(
        &mut self,
        l2block: L2Block,
        header_info: HeaderInfo,
        deposition_requests: Vec<DepositionRequest>,
    ) -> Result<Option<StartChallenge>> {
        let tip_number: u64 = self.local_state.tip.raw().number().unpack();
        assert!(
            l2block.raw().number().unpack() == tip_number + 1,
            "new l2block number must be the successor of the tip"
        );

        // process l2block
        let args = StateTransitionArgs {
            l2block: l2block.clone(),
            deposition_requests,
        };
        // process transactions
        if let Err(err) = self.generator.apply_state_transition(&mut self.store, args) {
            // handle tx error
            match err {
                GeneratorError::Transaction(err) => {
                    // TODO run offchain validator before send challenge, to make sure the block is bad
                    return Ok(Some(err.challenge_context));
                }
                err => return Err(err.into()),
            }
        }
        self.store
            .insert_block(l2block.clone(), header_info.clone())?;
        self.store.attach_block(l2block.clone())?;

        // update chain
        self.local_state.last_synced = header_info;
        self.local_state.tip = l2block;
        Ok(None)
    }

    /// Produce an unsigned new block
    ///
    /// This function should be called in the turn that the current aggregator to produce the next block,
    /// otherwise the produced block may invalided by the state-validator contract.
    pub fn produce_block(&mut self, param: ProduceBlockParam) -> Result<ProduceBlockResult> {
        let ProduceBlockParam {
            aggregator_id,
            tx_pool_pkg,
        } = param;

        // take txs from tx pool
        // produce block
        let parent_number: u64 = self.local_state.tip.raw().number().unpack();
        let number = parent_number + 1;
        let timestamp: u64 = unixtime()?;
        let submit_txs = {
            let tx_witness_root = calculate_merkle_root(
                tx_pool_pkg
                    .tx_receipts
                    .iter()
                    .map(|tx_recipt| &tx_recipt.tx_witness_hash)
                    .cloned()
                    .collect(),
            )
            .map_err(|err| anyhow!("merkle root error: {:?}", err))?;
            let tx_count = tx_pool_pkg.tx_receipts.len() as u32;
            let compacted_post_root_list: Vec<_> = tx_pool_pkg
                .tx_receipts
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
        let withdrawal_requests_root = calculate_merkle_root(
            tx_pool_pkg
                .withdrawal_requests
                .iter()
                .map(|request| request.raw().hash())
                .collect(),
        )
        .map_err(|err| anyhow!("merkle root error: {:?}", err))?;
        let prev_root: [u8; 32] = tx_pool_pkg.prev_account_state.root.into();
        let prev_account = AccountMerkleState::new_builder()
            .merkle_root(prev_root.pack())
            .count(tx_pool_pkg.prev_account_state.count.pack())
            .build();
        let post_root: [u8; 32] = tx_pool_pkg.post_account_state.root.into();
        let post_account = AccountMerkleState::new_builder()
            .merkle_root(post_root.pack())
            .count(tx_pool_pkg.post_account_state.count.pack())
            .build();
        let raw_block = RawL2Block::new_builder()
            .number(number.pack())
            .aggregator_id(aggregator_id.pack())
            .timestamp(timestamp.pack())
            .post_account(post_account.clone())
            .prev_account(prev_account)
            .withdrawal_requests_root(withdrawal_requests_root.pack())
            .submit_transactions(submit_txs)
            .build();
        // generate block fields from current state
        let kv_state: Vec<(H256, H256)> = tx_pool_pkg
            .touched_keys
            .iter()
            .map(|k| {
                self.store
                    .get_raw(k)
                    .map(|v| (*k, v))
                    .map_err(|err| anyhow!("can't fetch value error: {:?}", err))
            })
            .collect::<Result<_>>()?;
        let packed_kv_state = kv_state
            .iter()
            .map(|(k, v)| {
                let k: [u8; 32] = (*k).into();
                let v: [u8; 32] = (*v).into();
                (k, v)
            })
            .collect::<Vec<_>>()
            .pack();
        let proof = if kv_state.is_empty() {
            // nothing need to prove
            Vec::new()
        } else {
            self.store
                .account_smt()
                .merkle_proof(kv_state.iter().map(|(k, _v)| *k).collect())
                .map_err(|err| anyhow!("merkle proof error: {:?}", err))?
                .compile(kv_state)?
                .0
        };
        let txs: Vec<_> = tx_pool_pkg
            .tx_receipts
            .into_iter()
            .map(|tx| tx.tx)
            .collect();
        let block_proof = self
            .store
            .block_smt()
            .merkle_proof(vec![H256::from_u64(number)])
            .map_err(|err| anyhow!("merkle proof error: {:?}", err))?
            .compile(vec![(H256::from_u64(number), H256::zero())])?;
        let block = L2Block::new_builder()
            .raw(raw_block)
            .kv_state(packed_kv_state)
            .kv_state_proof(proof.pack())
            .transactions(txs.pack())
            .withdrawal_requests(tx_pool_pkg.withdrawal_requests.pack())
            .block_proof(block_proof.0.pack())
            .build();
        let post_block = {
            let post_block_root: [u8; 32] = block_proof
                .compute_root::<Blake2bHasher>(vec![(block.smt_key().into(), block.hash().into())])?
                .into();
            let block_count = number + 1;
            BlockMerkleState::new_builder()
                .merkle_root(post_block_root.pack())
                .count(block_count.pack())
                .build()
        };
        let global_state = GlobalState::new_builder()
            .account(post_account)
            .block(post_block)
            .build();
        Ok(ProduceBlockResult {
            block,
            global_state,
        })
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
