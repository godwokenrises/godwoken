use anyhow::{anyhow, Result};
use gw_common::{h256_ext::H256Ext, sparse_merkle_tree, state::State, H256};
use gw_generator::{
    generator::{StateTransitionArgs, StateTransitionResult},
    ChallengeContext, Generator,
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
    core::{ChallengeTargetType, Status},
    packed::{
        BlockMerkleState, CellInput, CellOutput, ChallengeTarget, ChallengeWitness, DepositRequest,
        GlobalState, L2Block, L2BlockCommittedInfo, RawL2Block, RollupConfig, Script, Transaction,
    },
    prelude::{Builder as GWBuilder, Entity as GWEntity, Pack as GWPack, Unpack as GWUnpack},
};
use parking_lot::Mutex;
use std::{convert::TryFrom, sync::Arc};

#[derive(Debug, Clone)]
pub struct ChallengeCell {
    pub input: CellInput,
    pub output: CellOutput,
    pub output_data: Bytes,
}

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
        cell: ChallengeCell,
        target: ChallengeTarget,
        witness: ChallengeWitness,
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
        cell: ChallengeCell,
        context: crate::challenge::VerifyContext,
    },
    // the rollup is in a challenge
    WaitChallenge {
        cell: ChallengeCell,
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

                        self.bad_blocks.push(l2block.clone());
                        log::info!("push new bad block {}", hex::encode(l2block.hash()));

                        // Update bad block proof, since block height changed
                        update_block_smt(db, &l2block)?;
                        let root: H256 = global_state.block().merkle_root().unpack();
                        update_bad_block_proof(db, root, bad_block_context)?;
                        log::info!("local bad bock proof updated");

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
                        log::info!("found a bad block 0x{}", hex::encode(l2block.hash()));

                        db.rollback()?;
                        log::info!("rollback db because of bad block found");

                        // Ensure block smt is updated to be able to build correct block proof. It doesn't
                        // matter current l2block is bad or not.
                        update_block_smt(db, &l2block)?;
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
                (
                    Status::Running,
                    L1ActionContext::Challenge {
                        cell,
                        target,
                        witness,
                    },
                ) => {
                    let status: u8 = global_state.status().into();
                    assert_eq!(Status::try_from(status), Ok(Status::Halting));

                    let local_bad_block = {
                        let bad_block_context = self.bad_block_context.as_ref();
                        bad_block_context.map(|ctx| ctx.witness.raw_l2block())
                    };

                    let challenge_block_number = witness.raw_l2block().number().unpack();
                    let local_bad_block_number =
                        local_bad_block.as_ref().map(|b| b.number().unpack());

                    {
                        let n = challenge_block_number;
                        let h = hex::encode::<[u8; 32]>(target.block_hash().unpack());
                        let i: u32 = target.target_index().unpack();
                        let t = ChallengeTargetType::try_from(target.target_type())
                            .map_err(|_| anyhow!("invalid challenge type"))?;
                        log::info!("sync challenge block {} 0x{} target {} {:?}", n, h, i, t);
                    }

                    // Challenge we can cancel:
                    // 1. no bad block found (aka self.bad_block_context is none)
                    // 2. challenge block number is smaller than local bad block
                    let local_tip_block_number = self.local_state.tip.raw().number().unpack();
                    if (local_bad_block.is_none()
                        && local_tip_block_number >= challenge_block_number)
                        || local_bad_block_number > Some(challenge_block_number)
                    {
                        log::info!("challenge cancelable, build verify context");

                        let generator = Arc::clone(&self.generator);
                        let context =
                            crate::challenge::build_verify_context(generator, db, &target)?;

                        return Ok(SyncEvent::BadChallenge { cell, context });
                    }

                    if local_bad_block.is_none() && local_tip_block_number < challenge_block_number
                    {
                        unreachable!("impossible challenge")
                    }

                    // Check block hash match
                    let challenge_block_hash: [u8; 32] = target.block_hash().unpack();
                    if local_bad_block_number == Some(challenge_block_number) {
                        let local_bad_block_hash = local_bad_block.map(|b| b.hash());
                        assert_eq!(local_bad_block_hash, Some(challenge_block_hash));
                    }

                    // Challenge block should be known
                    let mut bad_blocks = self.bad_blocks.iter();
                    let challenge_block_pos = bad_blocks
                        .position(|b| b.hash() == challenge_block_hash)
                        .expect("challenge unknown block");
                    let (_, reverted_blocks) = self.bad_blocks.split_at(challenge_block_pos);

                    // Either valid challenge or we don't have correct state to verify
                    // it (aka challenge block after our local bad block)
                    // If block is same, we don't care about target index and type, just want this
                    // block to be reverted.
                    let maybe_context = crate::challenge::build_revert_context(db, reverted_blocks);
                    // NOTE: Ensure db is rollback. build_revert_context will modify reverted_block_smt
                    // to compute merkle proof and root, so must rollback changes.
                    db.rollback()?;
                    log::info!("rollback db after prepare context for revert");

                    Ok(SyncEvent::WaitChallenge {
                        cell,
                        context: maybe_context?,
                    })
                }
                (Status::Halting, L1ActionContext::CancelChallenge) => {
                    let status: u8 = global_state.status().into();
                    assert_eq!(Status::try_from(status), Ok(Status::Running));

                    log::info!("challenge cancelled");

                    match self.bad_block_context {
                        // Previous challenge miss right target, we should challenge it
                        Some(ref bad_block) => Ok(SyncEvent::BadBlock {
                            context: bad_block.to_owned(),
                        }),
                        None => Ok(SyncEvent::Success),
                    }
                }
                (Status::Halting, L1ActionContext::Revert { reverted_blocks }) => {
                    let first_reverted_block = reverted_blocks.first().expect("first block");
                    let first_reverted_block_number = first_reverted_block.number().unpack();
                    log::debug!("first reverted block {}", first_reverted_block_number);

                    let status: u8 = global_state.status().into();
                    assert_eq!(Status::try_from(status), Ok(Status::Running));

                    // Valid block should not be reverted
                    let bad_block_context = self.bad_block_context.as_ref();
                    let local_bad_block = bad_block_context.map(|c| c.witness.raw_l2block());
                    let local_bad_block_number =
                        local_bad_block.as_ref().map(|b| b.number().unpack());
                    let local_bad_block_hash = local_bad_block.as_ref().map(|b| b.hash());

                    if local_bad_block.is_none()
                        || local_bad_block_number > Some(first_reverted_block_number)
                    {
                        if let Some(local_bad_block_hash) = local_bad_block_hash {
                            // First reverted block number is smaller than local bad block
                            let has_local_block = reverted_blocks
                                .iter()
                                .any(|b| b.hash() == local_bad_block_hash);

                            if !has_local_block {
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
                    // Since our local bad_blocks is ascended, simply compare hashes is ok
                    let pending_revert_blocks = self.bad_blocks.split_off(first_reverted_block_pos);
                    let pending_slice = pending_revert_blocks.iter().map(|b| b.raw().hash());
                    let reverted_slice = reverted_blocks.iter().map(|b| b.hash());
                    assert_eq!(
                        pending_slice.collect::<Vec<[u8; 32]>>(),
                        reverted_slice.collect::<Vec<[u8; 32]>>()
                    );

                    // Update reverted block smt
                    {
                        let mut reverted_block_smt = db.reverted_block_smt()?;
                        for reverted_block in pending_revert_blocks.iter() {
                            reverted_block_smt.update(reverted_block.hash().into(), H256::one())?;
                        }
                        let local_reverted_block_root = reverted_block_smt.root();

                        let global_reverted_block_root: H256 =
                            global_state.reverted_block_root().unpack();
                        assert_eq!(local_reverted_block_root, &global_reverted_block_root);

                        db.set_reverted_block_smt_root(*local_reverted_block_root)?;
                        log::debug!("update reverted block smt");
                    }

                    // Revert block smt (delete reverted block hashes)
                    {
                        let mut block_smt = db.block_smt()?;
                        for reverted_block in pending_revert_blocks.iter() {
                            block_smt.update(H256::from(reverted_block.smt_key()), H256::zero())?;
                        }
                        let block_smt_root = block_smt.root().to_owned();
                        let local_block_smt = BlockMerkleState::new_builder()
                            .merkle_root(Into::<[u8; 32]>::into(block_smt_root).pack())
                            .count(first_reverted_block.number())
                            .build();

                        let global_block_smt = global_state.block();
                        assert_eq!(local_block_smt.as_slice(), global_block_smt.as_slice());

                        db.set_block_smt_root(block_smt_root)?;
                        log::debug!("revert block smt");
                    }

                    // Update pending clearing blocks
                    self.pending_revert_blocks.extend(pending_revert_blocks);

                    // Revert account smt
                    {
                        let prev_account_smt = first_reverted_block.prev_account();
                        let global_account_smt = global_state.account();
                        assert_eq!(prev_account_smt.as_slice(), global_account_smt.as_slice());

                        let prev_account_root: H256 = prev_account_smt.merkle_root().unpack();
                        db.set_account_smt_root(prev_account_root)?;
                        db.set_account_count(prev_account_smt.count().unpack())?;
                        log::debug!("revert account smt");
                    }

                    // Revert tip block
                    {
                        let parent_block_hash: H256 =
                            first_reverted_block.parent_block_hash().unpack();
                        let global_tip_block_hash: H256 = global_state.tip_block_hash().unpack();
                        assert_eq!(parent_block_hash, global_tip_block_hash);

                        db.set_tip_block_hash(parent_block_hash)?;
                        log::debug!("revert tip block hash");

                        let parent_block = db
                            .get_block(&parent_block_hash)?
                            .expect("reverted parent block");

                        self.local_state.tip = parent_block;
                        log::debug!("revert chain local state tip block");
                    }

                    let local_tip_block_number = self.local_state.tip.raw().number();
                    log::info!("revert to block {}", local_tip_block_number.unpack());

                    // Check whether our bad block is reverted
                    if local_bad_block_hash == Some(first_reverted_block_hash) {
                        self.bad_block_context = None;
                        assert_eq!(self.bad_blocks.is_empty(), true);

                        log::info!("clear local bad block");
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
        log::debug!("last sync event {:?}", self.last_sync_event);

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
            log::debug!("commit db after sync");

            let tip_block_hash: H256 = self.local_state.tip.hash().into();
            if let SyncEvent::Success = self.last_sync_event {
                // update mem pool state
                self.mem_pool.lock().notify_new_tip(tip_block_hash)?;
            }

            // check consistency of account SMT
            let expected_account_root: H256 = {
                let raw_block = self.local_state.tip.raw();
                raw_block.post_account().merkle_root().unpack()
            };

            let state_db = StateDBTransaction::from_checkpoint(
                &db,
                CheckPoint::from_block_hash(&db, tip_block_hash, SubState::Block)?,
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
        // TODO: run offchain validator before send challenge, to make sure the block is bad
        let generator = &self.generator;
        let (tx_receipts, withdrawal_receipts) =
            match generator.verify_and_apply_state_transition(&chain_view, &mut tree, args) {
                StateTransitionResult::Success {
                    tx_receipts,
                    withdrawal_receipts,
                } => (tx_receipts, withdrawal_receipts),
                StateTransitionResult::Challenge { target, error } => {
                    log::debug!("verify and apply state transition error {}", error);

                    return Ok(Some(ChallengeContext {
                        target,
                        witness: build_challenge_witness(db, l2block.raw())?,
                    }));
                }
                StateTransitionResult::Generator(err) => return Err(err.into()),
            };

        // update chain
        db.insert_block(
            l2block.clone(),
            l2block_committed_info,
            global_state,
            tx_receipts,
            withdrawal_receipts,
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

fn update_block_smt(db: &StoreTransaction, block: &L2Block) -> Result<()> {
    let mut smt = db.block_smt()?;
    smt.update(block.smt_key().into(), block.hash().into())?;
    let root = smt.root();
    db.set_block_smt_root(*root)?;

    Ok(())
}
