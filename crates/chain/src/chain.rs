use anyhow::{anyhow, Result};
use gw_common::{sparse_merkle_tree, state::State, H256};
use gw_generator::{
    generator::StateTransitionArgs, ChallengeContext, Error as GeneratorError, Generator,
};
use gw_mem_pool::pool::MemPool;
use gw_store::{
    chain_view::ChainView,
    state_db::{StateDBTransaction, StateDBVersion},
    transaction::StoreTransaction,
    Store,
};
use gw_types::{
    bytes::Bytes,
    core::Status,
    packed::{
        ChallengeTarget, ChallengeWitness, DepositionRequest, GlobalState, L2Block,
        L2BlockCommittedInfo, L2BlockReader, RollupConfig, Script, Transaction, TxReceipt,
        VerifyTransactionWitness, WitnessArgs, WitnessArgsReader,
    },
    prelude::{
        Builder as GWBuilder, Entity as GWEntity, Pack as GWPack, Reader as GWReader,
        Unpack as GWUnpack,
    },
};
use parking_lot::Mutex;
use std::{convert::TryFrom, sync::Arc};

/// sync params
pub struct SyncParam {
    /// contains transitions from tip to fork point
    pub reverts: Vec<RevertedL1Action>,
    /// contains transitions from fork point to new tips
    pub updates: Vec<L1Action>,
}

#[derive(Debug, Eq, PartialEq, Clone)]
pub enum L1ActionContext {
    SubmitTxs {
        /// deposition requests
        deposition_requests: Vec<DepositionRequest>,
    },
    Challenge {
        context: ChallengeTarget,
    },
    CancelChallenge {
        context: VerifyTransactionWitness,
    },
    Revert {
        context: ChallengeTarget,
    },
}

#[derive(Debug, Eq, PartialEq, Clone)]
pub struct L1Action {
    /// transaction
    pub transaction: Transaction,
    /// l2block committed info
    pub l2block_committed_info: L2BlockCommittedInfo,
    pub context: L1ActionContext,
}

#[derive(Debug, Eq, PartialEq, Clone)]
pub struct RevertedL1Action {
    /// input global state
    pub prev_global_state: GlobalState,
    /// transaction
    pub transaction: Transaction,
    /// l2block committed info
    pub l2block_committed_info: L2BlockCommittedInfo,
    pub context: L1ActionContext,
}

/// sync method returned events
#[derive(Debug, Eq, PartialEq)]
pub enum SyncEvent {
    // success
    Success,
    // found a invalid block
    BadBlock(ChallengeContext),
    // found a invalid challenge
    BadChallenge {
        witness: VerifyTransactionWitness,
        tx_receipt: TxReceipt,
    },
    // the rollup is in a challenge
    WaitChallenge,
}

/// concrete type aliases
pub type StateStore = sparse_merkle_tree::default_store::DefaultStore<sparse_merkle_tree::H256>;

pub struct LocalState {
    tip: L2Block,
    last_synced: L2BlockCommittedInfo,
    last_global_state: GlobalState,
}

impl LocalState {
    pub fn tip(&self) -> &L2Block {
        &self.tip
    }

    pub fn status(&self) -> Status {
        let status: u8 = self.last_global_state.status().into();
        Status::try_from(status).expect("invalid status")
    }

    pub fn last_synced(&self) -> &L2BlockCommittedInfo {
        &self.last_synced
    }

    pub fn last_global_state(&self) -> &GlobalState {
        &self.last_global_state
    }
}

pub struct Chain {
    rollup_type_script_hash: [u8; 32],
    rollup_config_hash: [u8; 32],
    store: Store,
    bad_block_context: Option<ChallengeTarget>,
    local_state: LocalState,
    generator: Arc<Generator>,
    mem_pool: Arc<Mutex<MemPool>>,
}

impl Chain {
    pub fn create(
        rollup_config: &RollupConfig,
        rollup_type_script: &Script,
        store: Store,
        generator: Arc<Generator>,
        mem_pool: Arc<Mutex<MemPool>>,
    ) -> Result<Self> {
        // convert serde types to gw-types
        assert_eq!(
            rollup_config,
            &generator.rollup_context().rollup_config,
            "check generator rollup config"
        );
        let rollup_type_script_hash = rollup_type_script.hash();
        let chain_id: [u8; 32] = store.get_chain_id()?.into();
        assert_eq!(
            chain_id, rollup_type_script_hash,
            "Database chain_id must equals to rollup_script_hash"
        );
        let tip = store.get_tip_block()?;
        let last_synced = store
            .get_l2block_committed_info(&tip.hash().into())?
            .ok_or_else(|| anyhow!("can't find last synced committed info"))?;
        let last_global_state = store
            .get_block_post_global_state(&tip.hash().into())?
            .ok_or_else(|| anyhow!("can't find last global state"))?;
        let local_state = LocalState {
            tip,
            last_synced,
            last_global_state,
        };
        let rollup_config_hash = rollup_config.hash();
        Ok(Chain {
            store,
            bad_block_context: None,
            local_state,
            generator,
            mem_pool,
            rollup_type_script_hash,
            rollup_config_hash,
        })
    }

    /// return local state
    pub fn local_state(&self) -> &LocalState {
        &self.local_state
    }

    pub fn store(&self) -> &Store {
        &self.store
    }

    pub fn mem_pool(&self) -> &Mutex<MemPool> {
        &self.mem_pool
    }

    pub fn generator(&self) -> &Generator {
        &self.generator
    }

    pub fn rollup_config_hash(&self) -> &[u8; 32] {
        &self.rollup_config_hash
    }

    /// update a layer1 action
    fn update_l1action(&mut self, db: &StoreTransaction, action: L1Action) -> Result<SyncEvent> {
        let L1Action {
            transaction,
            l2block_committed_info,
            context,
        } = action;
        let global_state = parse_global_state(&transaction, &self.rollup_type_script_hash)?;
        assert!(
            {
                let number: u64 = l2block_committed_info.number().unpack();
                number
            } >= {
                let number: u64 = self.local_state.last_synced.number().unpack();
                number
            },
            "must be greater than or equalled to last synced number"
        );
        let status = {
            let status: u8 = self.local_state.last_global_state.status().into();
            Status::try_from(status).expect("invalid status")
        };
        let event = match (status, context) {
            (
                Status::Running,
                L1ActionContext::SubmitTxs {
                    deposition_requests,
                },
            ) => {
                // Submit transactions
                // parse layer2 block
                let l2block = parse_l2block(&transaction, &self.rollup_type_script_hash)?;
                let number: u64 = l2block.raw().number().unpack();
                if let Some(challenge_context) = self.process_block(
                    db,
                    l2block,
                    l2block_committed_info.clone(),
                    global_state.clone(),
                    deposition_requests,
                )? {
                    // stop syncing and return event
                    self.bad_block_context = Some(challenge_context.target.clone());
                    SyncEvent::BadBlock(challenge_context)
                } else {
                    println!("sync new block #{} success", number);
                    SyncEvent::Success
                }
            }
            (Status::Running, L1ActionContext::Challenge { context }) => {
                // Challenge
                let status: u8 = global_state.status().into();
                assert_eq!(Status::try_from(status), Ok(Status::Halting));
                if let Some(current_bad_block) = self.bad_block_context.as_ref() {
                    if current_bad_block.as_slice() == context.as_slice() {
                        // bad block is in challenge, just wait.
                        return Ok(SyncEvent::WaitChallenge);
                    }
                    SyncEvent::WaitChallenge
                } else {
                    // now, either we haven't found a bad block or the challenge is challenge a validate block
                    // in both cases the challenge is bad
                    // TODO: implement this
                    let _witness = VerifyTransactionWitness::default();
                    unimplemented!();
                    // SyncEvent::BadChallenge {
                    //     witness,
                    //     tx_receipt,
                    // }
                }
            }
            (Status::Halting, L1ActionContext::CancelChallenge { context: _ }) => {
                // TODO update states
                let status: u8 = global_state.status().into();
                assert_eq!(Status::try_from(status), Ok(Status::Running));
                SyncEvent::Success
            }
            (Status::Halting, L1ActionContext::Revert { context }) => {
                // TODO revert layer2 status
                let status: u8 = global_state.status().into();
                assert_eq!(Status::try_from(status), Ok(Status::Running));
                assert_eq!(
                    self.bad_block_context.as_ref().map(|b| b.as_slice()),
                    Some(context.as_slice()),
                    "revert from the bad block"
                );
                SyncEvent::Success
            }
            (status, context) => {
                panic!(
                    "unsupported syncing state: status {:?} context {:?}",
                    status, context
                );
            }
        };

        // update last global state
        self.local_state.last_global_state = global_state;
        self.local_state.last_synced = l2block_committed_info;
        Ok(event)
    }

    /// revert a layer1 action
    fn revert_l1action(&mut self, db: &StoreTransaction, action: RevertedL1Action) -> Result<()> {
        let RevertedL1Action {
            prev_global_state,
            transaction,
            l2block_committed_info,
            context,
        } = action;
        assert!(
            {
                let number: u64 = l2block_committed_info.number().unpack();
                number
            } <= {
                let number: u64 = self.local_state.last_synced.number().unpack();
                number
            },
            "must be smaller than or equalled to last synced number"
        );
        #[allow(clippy::single_match)]
        match context {
            L1ActionContext::SubmitTxs {
                deposition_requests: _,
            } => {
                // parse layer2 block
                let l2block = parse_l2block(&transaction, &self.rollup_type_script_hash)?;
                assert_eq!(
                    l2block.hash(),
                    self.local_state.tip.hash(),
                    "reverted l2block must be current tip"
                );
                db.detach_block(&l2block)?;

                // check reverted state
                {
                    // check tip block
                    let tip = db.get_tip_block()?;
                    let parent_block_hash: H256 = l2block.raw().parent_block_hash().unpack();
                    let tip_block_hash = tip.hash().into();
                    assert_eq!(parent_block_hash, tip_block_hash);
                    let l2block_number: u64 = l2block.raw().number().unpack();
                    let tip_number: u64 = tip.raw().number().unpack();
                    assert_eq!(l2block_number - 1, tip_number);

                    // check current state
                    let expected_state = l2block.raw().prev_account();
                    let state_db = StateDBTransaction::from_version(
                        &db,
                        StateDBVersion::from_history_state(&db, tip_block_hash, None)?,
                    )?;
                    let tree = state_db.account_state_tree()?;
                    let expected_root: H256 = expected_state.merkle_root().unpack();
                    let expected_count: u32 = expected_state.count().unpack();
                    assert_eq!(tree.calculate_root()?, expected_root);
                    assert_eq!(tree.get_account_count()?, expected_count);

                    // check genesis state still consistent
                    let script_hash = tree.get_script_hash(0)?;
                    assert!(!script_hash.is_zero());
                }
            }
            _ => {
                // do nothing
            }
        };

        // update last global state
        self.local_state.last_global_state = prev_global_state;
        self.local_state.tip = db.get_tip_block()?;
        self.local_state.last_synced = db
            .get_l2block_committed_info(&self.local_state.tip.hash().into())?
            .expect("last committed info");
        Ok(())
    }

    /// Sync chain from layer1
    pub fn sync(&mut self, param: SyncParam) -> Result<SyncEvent> {
        let db = self.store.begin_transaction();
        // revert layer1 actions
        if !param.reverts.is_empty() {
            // revert
            for reverted_action in param.reverts {
                self.revert_l1action(&db, reverted_action)?;
            }
        }
        // update layer1 actions
        for action in param.updates {
            let event = self.update_l1action(&db, action)?;
            // return to caller if any event happen
            if event != SyncEvent::Success {
                db.commit()?;
                return Ok(event);
            }
        }
        db.commit()?;
        // update mem pool state
        self.mem_pool
            .lock()
            .notify_new_tip(self.local_state.tip.hash().into())?;
        // check consistency of account SMT
        {
            // check account SMT, should be able to calculate account state root
            let expected_account_root: H256 = self
                .local_state
                .tip
                .raw()
                .post_account()
                .merkle_root()
                .unpack();
            let state_db = StateDBTransaction::from_version(
                &db,
                StateDBVersion::from_history_state(
                    &db,
                    self.local_state.tip().hash().into(),
                    None,
                )?,
            )?;
            assert_eq!(
                state_db.account_smt().unwrap().root(),
                &expected_account_root,
                "account root consistent in DB"
            );
            let tree = state_db.account_state_tree()?;
            let current_account_root = tree.calculate_root().unwrap();
            assert_eq!(
                current_account_root, expected_account_root,
                "check account tree"
            );
        }
        Ok(SyncEvent::Success)
    }

    fn process_block(
        &mut self,
        db: &StoreTransaction,
        l2block: L2Block,
        l2block_committed_info: L2BlockCommittedInfo,
        global_state: GlobalState,
        deposition_requests: Vec<DepositionRequest>,
    ) -> Result<Option<ChallengeContext>> {
        let tip_number: u64 = self.local_state.tip.raw().number().unpack();
        let tip_block_hash = self.local_state.tip.raw().hash();
        let block_number: u64 = l2block.raw().number().unpack();
        assert_eq!(
            {
                let parent_block_hash: [u8; 32] = l2block.raw().parent_block_hash().unpack();
                (block_number, parent_block_hash)
            },
            (tip_number + 1, tip_block_hash),
            "new l2block must be the successor of the tip"
        );

        // process l2block
        let args = StateTransitionArgs {
            l2block: l2block.clone(),
            deposition_requests: deposition_requests.clone(),
        };
        let tip_block_hash = self.local_state.tip().hash().into();
        let chain_view = ChainView::new(db, tip_block_hash);
        let state_db = StateDBTransaction::from_version(
            db,
            StateDBVersion::from_future_state(block_number, 0),
        )?;
        let mut tree = state_db.account_state_tree()?;
        // process transactions
        let result = match self
            .generator
            .apply_state_transition(&chain_view, &mut tree, args)
        {
            Ok(result) => result,
            Err(err) => {
                // handle tx error
                match err {
                    GeneratorError::Transaction(err) => {
                        // TODO run offchain validator before send challenge, to make sure the block is bad
                        let block_hash: [u8; 32] = err.context.block_hash().unpack();
                        let block_proof = db
                            .block_smt()?
                            .merkle_proof(vec![l2block.smt_key().into()])?
                            .compile(vec![(l2block.smt_key().into(), block_hash.into())])?;
                        let witness = ChallengeWitness::new_builder()
                            .raw_l2block(l2block.raw())
                            .block_proof(block_proof.0.pack())
                            .build();
                        let context = ChallengeContext {
                            target: err.context,
                            witness,
                        };
                        return Ok(Some(context));
                    }
                    err => return Err(err.into()),
                }
            }
        };

        // update chain
        db.insert_block(
            l2block.clone(),
            l2block_committed_info,
            global_state,
            result.receipts,
            deposition_requests,
        )?;
        db.attach_block(l2block.clone())?;
        tree.submit_tree()?;
        let post_merkle_root: H256 = l2block.raw().post_account().merkle_root().unpack();
        assert_eq!(
            tree.calculate_root()?,
            post_merkle_root,
            "post account merkle root must be consistent"
        );
        self.local_state.tip = l2block;
        Ok(None)
    }
}

fn parse_global_state(tx: &Transaction, rollup_id: &[u8; 32]) -> Result<GlobalState> {
    // find rollup state cell from outputs
    let (i, _) = tx
        .raw()
        .outputs()
        .into_iter()
        .enumerate()
        .find(|(_i, output)| {
            output.type_().to_opt().map(|type_| type_.hash()).as_ref() == Some(rollup_id)
        })
        .ok_or_else(|| anyhow!("no rollup cell found"))?;

    let output_data: Bytes = tx
        .raw()
        .outputs_data()
        .get(i)
        .ok_or_else(|| anyhow!("no output data"))?
        .unpack();
    GlobalState::from_slice(&output_data).map_err(|_| anyhow!("global state unpacking error"))
}

fn parse_l2block(tx: &Transaction, rollup_id: &[u8; 32]) -> Result<L2Block> {
    // find rollup state cell from outputs
    let (i, _) = tx
        .raw()
        .outputs()
        .into_iter()
        .enumerate()
        .find(|(_i, output)| {
            output.type_().to_opt().map(|type_| type_.hash()).as_ref() == Some(rollup_id)
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
