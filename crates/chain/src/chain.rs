use anyhow::{anyhow, Result};
use gw_common::{sparse_merkle_tree, state::State, H256};
use gw_generator::{
    generator::StateTransitionArgs, ChallengeContext, Error as GeneratorError, Generator,
};
use gw_mem_pool::pool::MemPool;
use gw_store::{
    chain_view::ChainView,
    state_db::{CheckPoint, StateDBMode, StateDBTransaction, SubState, WriteContext},
    transaction::StoreTransaction,
    Store,
};
use gw_types::{
    bytes::Bytes,
    core::Status,
    packed::{
        ChallengeWitness, DepositRequest, GlobalState, L2Block, L2BlockCommittedInfo, RawL2Block,
        RollupConfig, Script, Transaction,
    },
    prelude::{Builder as GWBuilder, Entity as GWEntity, Pack as GWPack, Unpack as GWUnpack},
};
use parking_lot::Mutex;
use std::{convert::TryFrom, sync::Arc};

/// sync params
#[derive(Clone)]
pub struct SyncParam {
    /// contains transitions from tip to fork point
    pub reverts: Vec<RevertedL1Action>,
    /// contains transitions from fork point to new tips
    pub updates: Vec<L1Action>,
}

#[derive(Debug, Clone)]
pub enum L1ActionContext {
    SubmitBlock {
        /// deposit requests
        l2block: L2Block,
        deposit_requests: Vec<DepositRequest>,
        reverted_block_hashes: Vec<[u8; 32]>,
    },
    Challenge {
        context: ChallengeContext,
    },
    CancelChallenge,
    Revert {
        reverted_blocks: Vec<RawL2Block>,
    },
}

#[derive(Debug, Clone)]
pub struct L1Action {
    /// transaction
    pub transaction: Transaction,
    /// l2block committed info
    pub l2block_committed_info: L2BlockCommittedInfo,
    pub context: L1ActionContext,
}

#[derive(Debug, Clone)]
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
#[derive(Debug, Clone)]
pub enum SyncEvent {
    // success
    Success,
    // found a invalid block
    BadBlock {
        context: ChallengeContext,
    },
    // found a invalid challenge
    BadChallenge {
        context: crate::challenge::VerifyContext,
    },
    // the rollup is in a challenge
    WaitChallenge {
        context: crate::challenge::RevertContext,
    },
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
    bad_block_context: Option<ChallengeContext>,
    bad_blocks: Vec<L2Block>,
    pending_revert_blocks: Vec<L2Block>,
    last_sync_event: SyncEvent,
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
            bad_blocks: Vec::new(),
            pending_revert_blocks: Vec::new(),
            last_sync_event: SyncEvent::Success,
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

    pub fn rollup_type_script_hash(&self) -> &[u8; 32] {
        &self.rollup_type_script_hash
    }

    pub fn pending_revert_blocks(&self) -> &[L2Block] {
        &self.pending_revert_blocks
    }

    pub fn last_sync_event(&self) -> &SyncEvent {
        &self.last_sync_event
    }

    /// update a layer1 action
    fn update_l1action(&mut self, db: &StoreTransaction, action: L1Action) -> Result<()> {
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

        let update = || -> Result<SyncEvent> {
            match (status, context) {
                (
                    Status::Running,
                    L1ActionContext::SubmitBlock {
                        l2block,
                        deposit_requests,
                        reverted_block_hashes,
                    },
                ) => {
                    // Clear reverted l2blocks. It doesn't matter current l2block is bad or not, since
                    // custodian and withdrawal are reverted on chain.
                    self.pending_revert_blocks
                        .retain(|block| !reverted_block_hashes.contains(&block.hash()));

                    // If there's new l2block after bad block, also mark it bad block since it
                    // bases on incorrect state.
                    let l2block_number: u64 = l2block.raw().number().unpack();
                    if let Some(ref mut bad_block_context) = self.bad_block_context {
                        let last_bad_block_nubmer = {
                            let last_bad_block = self.bad_blocks.last();
                            let to_number = last_bad_block.map(|b| b.raw().number().unpack());
                            to_number.expect("last bad block should exists")
                        };

                        let should_be_next_bad_block =
                            last_bad_block_nubmer.saturating_add(1) == l2block_number;
                        // Panic means reverted blocks aren't correctly move to
                        // pending_revert_blocks.
                        assert_eq!(should_be_next_bad_block, true);

                        log::debug!("push last bad block, hash {:?}", l2block.hash());
                        self.bad_blocks.push(l2block.clone());

                        // Update bad block proof, since block height changed
                        let root: [u8; 32] = global_state.block().merkle_root().unpack();
                        db.block_smt()?
                            .update(l2block.smt_key().into(), l2block.hash().into())?;
                        update_bad_block_proof(db, root.into(), bad_block_context)?;

                        return Ok(SyncEvent::BadBlock {
                            context: bad_block_context.to_owned(),
                        });
                    }

                    if let Some(challenge_context) = self.process_block(
                        db,
                        l2block.clone(),
                        l2block_committed_info.clone(),
                        global_state.clone(),
                        deposit_requests,
                    )? {
                        log::info!("a bad block found, hash {:?}", l2block.hash());

                        db.rollback()?;

                        // Ensure block smt is updated to be able to build correct block proof. It doesn't
                        // matter current l2block is bad or not.
                        db.block_smt()?
                            .update(l2block.smt_key().into(), l2block.hash().into())?;

                        // stop syncing and return event
                        self.bad_block_context = Some(challenge_context.clone());

                        assert_eq!(self.bad_blocks.is_empty(), true);
                        self.bad_blocks.push(l2block);

                        Ok(SyncEvent::BadBlock {
                            context: challenge_context,
                        })
                    } else {
                        log::info!("sync new block #{} success", l2block_number);
                        Ok(SyncEvent::Success)
                    }
                }
                (Status::Running, L1ActionContext::Challenge { context }) => {
                    let status: u8 = global_state.status().into();
                    assert_eq!(Status::try_from(status), Ok(Status::Halting));

                    let local_bad_block = {
                        let bad_block_context = self.bad_block_context.as_ref();
                        bad_block_context.map(|ctx| ctx.witness.raw_l2block())
                    };

                    // Challenge we can cancel:
                    // 1. no bad block found (aka self.bad_block_context is none)
                    // 2. challenge block number is smaller than local bad block
                    let challenge_block_number = context.witness.raw_l2block().number().unpack();
                    let local_block_number = local_bad_block.as_ref().map(|b| b.number().unpack());
                    if local_bad_block.is_none()
                        || local_block_number > Some(challenge_block_number)
                    {
                        use crate::challenge::build_verify_context;
                        let generator = Arc::clone(&self.generator);

                        return Ok(SyncEvent::BadChallenge {
                            context: build_verify_context(generator, db, &context.target)?,
                        });
                    }

                    // Check forked
                    let challenge_block_hash: [u8; 32] = context.target.block_hash().unpack();
                    if local_block_number == Some(challenge_block_number) {
                        let local_block_hash = local_bad_block.map(|lb| lb.hash()).expect("exists");
                        assert_eq!(local_block_hash, challenge_block_hash, "challenge forked");
                    }

                    // Challenge block should be known
                    let mut bad_blocks = self.bad_blocks.iter();
                    let challenge_block_pos = bad_blocks
                        .position(|b| b.hash() == challenge_block_hash)
                        .expect("challenge unknown block");
                    let (_, reverted_blocks) = self.bad_blocks.split_at(challenge_block_pos);

                    // Either valid challenge or we don't have correct state to verify
                    // it (aka challenge block after our caught bad block)
                    // If block is same, we don't care about target index and type, just want this
                    // block to be reverted.
                    // let context = crate::challenge::build_revert_context(db, )
                    use crate::challenge::build_revert_context;
                    Ok(SyncEvent::WaitChallenge {
                        context: build_revert_context(db, reverted_blocks)?,
                    })
                }
                (Status::Halting, L1ActionContext::CancelChallenge) => {
                    let status: u8 = global_state.status().into();
                    assert_eq!(Status::try_from(status), Ok(Status::Running));

                    // TODO: If block hash matched, forked
                    match self.bad_block_context {
                        // Previous challenge miss right target, we should challenge it
                        Some(ref bad_block) => Ok(SyncEvent::BadBlock {
                            context: bad_block.to_owned(),
                        }),
                        None => Ok(SyncEvent::Success),
                    }
                }
                (Status::Halting, L1ActionContext::Revert { reverted_blocks }) => {
                    let first_reverted_block =
                        reverted_blocks.first().expect("first block not found");
                    let first_reverted_block_number = first_reverted_block.number().unpack();
                    log::info!("first reverted block {}", first_reverted_block_number);

                    let status: u8 = global_state.status().into();
                    assert_eq!(Status::try_from(status), Ok(Status::Running));

                    // Valid block should not be reverted
                    let bad_block_context = self.bad_block_context.as_ref();
                    let caught_bad_block = bad_block_context.map(|c| c.witness.raw_l2block());
                    let caught_bad_block_number =
                        caught_bad_block.as_ref().map(|b| b.number().unpack());
                    let caught_bad_block_hash = caught_bad_block.as_ref().map(|b| b.hash());
                    if caught_bad_block.is_none()
                        || caught_bad_block_number > Some(first_reverted_block_number)
                    {
                        if let Some(caught_bad_block_hash) = caught_bad_block_hash {
                            // First reverted block number is smaller than caught bad block
                            let find_caught_block = reverted_blocks
                                .iter()
                                .find(|b| b.hash() == caught_bad_block_hash);

                            if find_caught_block.is_none() {
                                panic!("chain forked");
                            }
                        }
                        panic!("a valid block is reverted");
                    }

                    // First reverted blocks should be known, otherwise forked
                    let mut bad_blocks = self.bad_blocks.iter();
                    let first_reverted_block_hash = first_reverted_block.hash();
                    let first_reverted_block_pos = bad_blocks
                        .position(|b| b.hash() == first_reverted_block_hash)
                        .expect("first reverted block should be known");

                    // Both bad blocks and reverted_blocks should be ascended and matched
                    let pending_revert_blocks = self.bad_blocks.split_off(first_reverted_block_pos);
                    let pending_slice = pending_revert_blocks.iter().map(|b| b.raw().hash());
                    let reverted_slice = reverted_blocks.iter().map(|b| b.hash());
                    assert_eq!(
                        pending_slice.collect::<Vec<[u8; 32]>>(),
                        reverted_slice.collect::<Vec<[u8; 32]>>()
                    );

                    // Update reverted block smt
                    let mut reverted_block_smt = db.reverted_block_smt()?;
                    for reverted_block in pending_revert_blocks.iter() {
                        reverted_block_smt.update(
                            reverted_block.smt_key().into(),
                            reverted_block.hash().into(),
                        )?;
                    }
                    let global_state_reverted_block_root: [u8; 32] =
                        global_state.reverted_block_root().unpack();
                    assert_eq!(
                        reverted_block_smt.root(),
                        &global_state_reverted_block_root.into()
                    );

                    // Update pending clearing blocks
                    self.pending_revert_blocks.extend(pending_revert_blocks);

                    // Check whether our bad block is reverted
                    if caught_bad_block_hash == Some(first_reverted_block_hash) {
                        self.bad_block_context = None;
                        assert_eq!(self.bad_blocks.is_empty(), true);
                    }

                    match self.bad_block_context {
                        // If our bad block isn't reverted, just challenge it
                        Some(ref mut bad_block_context) => {
                            // Update bad block proof since block height changed
                            let root: [u8; 32] = global_state.block().merkle_root().unpack();
                            update_bad_block_proof(db, root.into(), bad_block_context)?;

                            Ok(SyncEvent::BadBlock {
                                context: bad_block_context.to_owned(),
                            })
                        }
                        None => Ok(SyncEvent::Success),
                    }
                }
                (status, context) => {
                    panic!(
                        "unsupported syncing state: status {:?} context {:?}",
                        status, context
                    );
                }
            }
        };

        self.last_sync_event = update()?;
        self.local_state.last_global_state = global_state;
        self.local_state.last_synced = l2block_committed_info;

        Ok(())
    }

    /// revert a layer1 action
    fn revert_l1action(&mut self, db: &StoreTransaction, action: RevertedL1Action) -> Result<()> {
        let RevertedL1Action {
            prev_global_state,
            transaction: _,
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
            L1ActionContext::SubmitBlock {
                l2block,
                deposit_requests: _,
                reverted_block_hashes: _,
            } => {
                assert_eq!(
                    l2block.hash(),
                    self.local_state.tip.hash(),
                    "reverted l2block must be current tip"
                );
                let rollup_config = &self.generator().rollup_context().rollup_config;
                db.detach_block(&l2block, rollup_config)?;

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
                    let state_db = StateDBTransaction::from_checkpoint(
                        &db,
                        CheckPoint::from_block_hash(&db, tip_block_hash, SubState::Block)?,
                        StateDBMode::ReadOnly,
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
    pub fn sync(&mut self, param: SyncParam) -> Result<()> {
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
            self.update_l1action(&db, action)?;
            db.commit()?;

            if let SyncEvent::Success = self.last_sync_event {
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
                    let state_db = StateDBTransaction::from_checkpoint(
                        &db,
                        CheckPoint::from_block_hash(
                            &db,
                            self.local_state.tip().hash().into(),
                            SubState::Block,
                        )?,
                        StateDBMode::ReadOnly,
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
            }
        }

        Ok(())
    }

    fn process_block(
        &mut self,
        db: &StoreTransaction,
        l2block: L2Block,
        l2block_committed_info: L2BlockCommittedInfo,
        global_state: GlobalState,
        deposit_requests: Vec<DepositRequest>,
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
            deposit_requests: deposit_requests.clone(),
        };
        let tip_block_hash = self.local_state.tip().hash().into();
        let chain_view = ChainView::new(db, tip_block_hash);
        let state_db = StateDBTransaction::from_checkpoint(
            db,
            CheckPoint::new(block_number, SubState::Block),
            StateDBMode::Write(WriteContext::new(l2block.withdrawals().len() as u32)),
        )?;
        let mut tree = state_db.account_state_tree()?;

        let prev_merkle_root: H256 = l2block.raw().prev_account().merkle_root().unpack();
        assert_eq!(
            tree.calculate_root()?,
            prev_merkle_root,
            "prev account merkle root must be consistent"
        );

        // process transactions
        let generator = &self.generator;
        let result = match generator.verify_and_apply_state_transition(&chain_view, &mut tree, args)
        {
            Ok(result) => result,
            // TODO: run offchain validator before send challenge, to make sure the block is bad
            Err(GeneratorError::WithdrawalWithContext(err)) => {
                return Ok(Some(ChallengeContext {
                    target: err.context,
                    witness: build_challenge_witness(db, l2block.raw())?,
                }));
            }
            Err(GeneratorError::Transaction(err)) => {
                return Ok(Some(ChallengeContext {
                    target: err.context,
                    witness: build_challenge_witness(db, l2block.raw())?,
                }));
            }
            Err(err) => return Err(err.into()),
        };

        // update chain
        db.insert_block(
            l2block.clone(),
            l2block_committed_info,
            global_state,
            result.tx_receipts,
            result.withdrawal_receipts,
            deposit_requests,
        )?;
        let rollup_config = &self.generator.rollup_context().rollup_config;
        db.attach_block(l2block.clone(), rollup_config)?;
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

fn build_challenge_witness(
    db: &StoreTransaction,
    raw_l2block: RawL2Block,
) -> Result<ChallengeWitness> {
    let block_proof = db
        .block_smt()?
        .merkle_proof(vec![raw_l2block.smt_key().into()])?
        .compile(vec![(
            raw_l2block.smt_key().into(),
            raw_l2block.hash().into(),
        )])?;

    Ok(ChallengeWitness::new_builder()
        .raw_l2block(raw_l2block)
        .block_proof(block_proof.0.pack())
        .build())
}

fn update_bad_block_proof(
    db: &StoreTransaction,
    global_state_block_root: H256,
    bad_block_context: &mut ChallengeContext,
) -> Result<()> {
    let block_smt = db.block_smt()?;
    let root = block_smt.root();
    assert_eq!(root, &global_state_block_root);

    let raw_block = bad_block_context.witness.raw_l2block();
    let block_proof = block_smt
        .merkle_proof(vec![raw_block.smt_key().into()])?
        .compile(vec![(raw_block.smt_key().into(), raw_block.hash().into())])?;

    // Update proof
    let updated_witness = {
        let old_witness = bad_block_context.witness.clone();
        let to_builder = old_witness.as_builder();
        to_builder.block_proof(block_proof.0.pack()).build()
    };

    *bad_block_context = ChallengeContext {
        target: bad_block_context.target.to_owned(),
        witness: updated_witness,
    };

    Ok(())
}
