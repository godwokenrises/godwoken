#![allow(clippy::mutable_key_type)]

use anyhow::{anyhow, bail, Context, Result};
use gw_challenge::offchain::{verify_tx::TxWithContext, OffChainMockContext};
use gw_common::{sparse_merkle_tree, state::State, CKB_SUDT_SCRIPT_ARGS, H256};
use gw_config::ChainConfig;
use gw_generator::{
    generator::{ApplyBlockArgs, ApplyBlockResult},
    traits::StateExt,
    types::vm::ChallengeContext,
    Generator,
};
use gw_jsonrpc_types::debugger::ReprMockTransaction;
use gw_mem_pool::pool::MemPool;
use gw_store::{
    chain_view::ChainView, state::state_db::StateContext, traits::chain_store::ChainStore,
    transaction::StoreTransaction, Store,
};
use gw_types::{
    bytes::Bytes,
    core::Status,
    offchain::global_state_from_slice,
    packed::{
        BlockMerkleState, Byte32, CellInput, CellOutput, ChallengeTarget, ChallengeWitness,
        DepositInfoVec, GlobalState, L2Block, NumberHash, RawL2Block, RollupConfig, Script,
        Transaction, WithdrawalRequestExtra,
    },
    prelude::{Builder as GWBuilder, Entity as GWEntity, Pack as GWPack, Unpack as GWUnpack},
};
use std::{collections::HashSet, convert::TryFrom, sync::Arc, time::Instant};
use tokio::sync::Mutex;
use tracing::instrument;

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
        deposit_info_vec: DepositInfoVec,
        deposit_asset_scripts: HashSet<Script>,
        withdrawals: Vec<WithdrawalRequestExtra>,
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
    pub context: L1ActionContext,
}

#[derive(Debug, Clone)]
pub enum RevertL1ActionContext {
    SubmitValidBlock { l2block: L2Block },
    RewindToLastValidTip,
}

#[derive(Debug, Clone)]
pub struct RevertedL1Action {
    /// input global state
    pub prev_global_state: GlobalState,
    pub context: RevertL1ActionContext,
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
        context: Box<gw_challenge::types::VerifyContext>,
    },
    // the rollup is in a challenge
    WaitChallenge {
        cell: ChallengeCell,
        context: gw_challenge::types::RevertContext,
    },
}

impl SyncEvent {
    pub fn is_success(&self) -> bool {
        matches!(self, SyncEvent::Success)
    }
}

/// concrete type aliases
pub type StateStore = sparse_merkle_tree::default_store::DefaultStore<sparse_merkle_tree::H256>;

pub struct LocalState {
    tip: L2Block,
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

    pub fn last_global_state(&self) -> &GlobalState {
        &self.last_global_state
    }
}

pub struct Chain {
    rollup_type_script_hash: [u8; 32],
    rollup_config_hash: [u8; 32],
    store: Store,
    challenge_target: Option<ChallengeTarget>,
    last_sync_event: SyncEvent,
    local_state: LocalState,
    generator: Arc<Generator>,
    mem_pool: Option<Arc<Mutex<MemPool>>>,
    skipped_invalid_block_list: HashSet<H256>,
}

impl Chain {
    pub fn create(
        rollup_config: &RollupConfig,
        rollup_type_script: &Script,
        config: &ChainConfig,
        store: Store,
        generator: Arc<Generator>,
        mem_pool: Option<Arc<Mutex<MemPool>>>,
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
        let last_global_state = store
            .get_block_post_global_state(&tip.hash().into())?
            .ok_or_else(|| anyhow!("can't find last global state"))?;
        let local_state = LocalState {
            tip,
            last_global_state,
        };
        let rollup_config_hash = rollup_config.hash();
        let skipped_invalid_block_list = config
            .skipped_invalid_block_list
            .iter()
            .cloned()
            .map(|ckb_h256| {
                let h: [u8; 32] = ckb_h256.into();
                h.into()
            })
            .collect();
        Ok(Chain {
            store,
            challenge_target: None,
            last_sync_event: SyncEvent::Success,
            local_state,
            generator,
            mem_pool,
            rollup_type_script_hash,
            rollup_config_hash,
            skipped_invalid_block_list,
        })
    }

    /// return local state
    pub fn local_state(&self) -> &LocalState {
        &self.local_state
    }

    pub fn store(&self) -> &Store {
        &self.store
    }

    pub fn mem_pool(&self) -> &Option<Arc<Mutex<MemPool>>> {
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

    pub fn last_sync_event(&self) -> &SyncEvent {
        &self.last_sync_event
    }

    pub fn bad_block_hash(&self) -> Option<H256> {
        self.challenge_target
            .as_ref()
            .map(|t| t.block_hash().unpack())
    }

    pub fn dump_cancel_challenge_tx(
        &self,
        offchain_mock_context: &OffChainMockContext,
        target: ChallengeTarget,
    ) -> Result<ReprMockTransaction> {
        let db = self.store().begin_transaction();

        let verify_context =
            gw_challenge::context::build_verify_context(Arc::clone(&self.generator), &db, &target)
                .with_context(|| "dump cancel challenge tx from chain")?;

        let global_state = {
            let get_state = db.get_block_post_global_state(&target.block_hash().unpack())?;
            let state = get_state
                .ok_or_else(|| anyhow!("post global state for challenge target {:?}", target))?;
            let to_builder = state.as_builder().status((Status::Halting as u8).into());
            to_builder.build()
        };

        let mock_output = gw_challenge::offchain::mock_cancel_challenge_tx(
            &offchain_mock_context.mock_rollup,
            global_state,
            target,
            verify_context,
            None,
        )
        .with_context(|| "dump cancel challenge tx from chain")?;

        gw_challenge::offchain::dump_tx(
            &offchain_mock_context.rollup_cell_deps,
            TxWithContext::from(mock_output),
        )
    }

    /// update a layer1 action
    fn update_l1action(&mut self, db: &StoreTransaction, action: L1Action) -> Result<()> {
        let L1Action {
            transaction,
            context,
        } = action;
        let global_state = parse_global_state(&transaction, &self.rollup_type_script_hash)?;
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
                        deposit_info_vec,
                        deposit_asset_scripts,
                        withdrawals,
                    },
                ) => {
                    let local_tip = self.local_state.tip();
                    let parent_block_hash: [u8; 32] = l2block.raw().parent_block_hash().unpack();
                    if parent_block_hash != local_tip.hash() {
                        return Err(anyhow!("fork detected"));
                    }

                    // Reverted block root should not change
                    let local_reverted_block_root = db.get_reverted_block_smt_root()?;
                    let global_reverted_block_root: H256 =
                        global_state.reverted_block_root().unpack();
                    assert_eq!(local_reverted_block_root, global_reverted_block_root);

                    // Check bad block challenge target
                    let challenge_target =
                        db.get_bad_block_challenge_target(&l2block.hash().into())?;
                    if self.challenge_target.is_none() && challenge_target.is_some() {
                        self.challenge_target = challenge_target;
                    }

                    if let Some(ref target) = self.challenge_target {
                        db.insert_bad_block(&l2block, &global_state)?;
                        log::info!("insert bad block 0x{}", hex::encode(l2block.hash()));

                        let global_block_root: H256 = global_state.block().merkle_root().unpack();
                        let local_block_root = db.get_block_smt_root()?;
                        assert_eq!(local_block_root, global_block_root, "block root fork");

                        self.local_state.tip = l2block;

                        let context =
                            gw_challenge::context::build_challenge_context(db, target.to_owned())?;
                        return Ok(SyncEvent::BadBlock { context });
                    }

                    if let Some(challenge_target) = self.process_block(
                        db,
                        l2block.clone(),
                        global_state.clone(),
                        deposit_info_vec,
                        deposit_asset_scripts,
                        withdrawals,
                    )? {
                        db.rollback()?;

                        let block_number = l2block.raw().number().unpack();
                        log::warn!("bad block #{} found, rollback db", block_number,);

                        db.insert_bad_block(&l2block, &global_state)?;
                        log::info!("insert bad block 0x{}", hex::encode(l2block.hash()));

                        let global_block_root: H256 = global_state.block().merkle_root().unpack();
                        let local_block_root = db.get_block_smt_root()?;
                        assert_eq!(local_block_root, global_block_root, "block root fork");

                        assert!(self.challenge_target.is_none());
                        db.set_bad_block_challenge_target(
                            &l2block.hash().into(),
                            &challenge_target,
                        )?;
                        self.challenge_target = Some(challenge_target.clone());
                        self.local_state.tip = l2block;

                        let context =
                            gw_challenge::context::build_challenge_context(db, challenge_target)?;
                        Ok(SyncEvent::BadBlock { context })
                    } else {
                        let block_number = l2block.raw().number().unpack();
                        let nh = NumberHash::new_builder()
                            .number(l2block.raw().number())
                            .block_hash(l2block.hash().pack())
                            .build();

                        self.calculate_and_store_finalized_custodians(db, block_number)?;
                        db.set_last_submitted_block_number_hash(&nh.as_reader())?;
                        db.set_last_confirmed_block_number_hash(&nh.as_reader())?;
                        db.set_block_submit_tx(block_number, &transaction.as_reader())?;

                        log::info!("sync new block #{} success", block_number);

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

                    let global_block_root: H256 = global_state.block().merkle_root().unpack();
                    if global_block_root != db.get_block_smt_root()? {
                        return Err(anyhow!("fork detected"));
                    }

                    let challenge_block_number = witness.raw_l2block().number().unpack();
                    let local_bad_block_number = {
                        let block_hash: Option<H256> = self.bad_block_hash();
                        let to_number = block_hash.map(|hash| db.get_block_number(&hash));
                        to_number.transpose()?.flatten()
                    };

                    // Challenge we can cancel:
                    // 1. no bad block found (aka self.bad_block is none)
                    // 2. challenge block number is smaller than local bad block
                    let local_tip_block_number = self.local_state.tip.raw().number().unpack();
                    if (self.challenge_target.is_none()
                        && local_tip_block_number >= challenge_block_number)
                        || local_bad_block_number > Some(challenge_block_number)
                    {
                        log::info!("challenge cancelable, build verify context");

                        let generator = Arc::clone(&self.generator);
                        let context = Box::new(gw_challenge::context::build_verify_context(
                            generator, db, &target,
                        )?);

                        return Ok(SyncEvent::BadChallenge { cell, context });
                    }

                    if self.challenge_target.is_none()
                        && local_tip_block_number < challenge_block_number
                    {
                        unreachable!("impossible challenge")
                    }

                    // Now either a valid challenge or we don't have correct state to verify
                    // it (aka challenge block after our local bad block)
                    // If block is same, we don't care about target index and type, just want this
                    // bad block to be reverted anyway.
                    let revert_blocks = package_bad_blocks(db, &target.block_hash().unpack())?;
                    let context = gw_challenge::context::build_revert_context(db, &revert_blocks)?;
                    // NOTE: Ensure db is rollback. build_revert_context will modify reverted_block_smt
                    // to compute merkle proof and root, so must rollback changes.
                    db.rollback()?;
                    log::info!("rollback db after prepare context for revert");

                    Ok(SyncEvent::WaitChallenge { cell, context })
                }
                (Status::Halting, L1ActionContext::CancelChallenge) => {
                    let status: u8 = global_state.status().into();
                    assert_eq!(Status::try_from(status), Ok(Status::Running));

                    log::info!("challenge cancelled");
                    match self.challenge_target {
                        // Previous challenge miss right target, we should challenge it
                        Some(ref target) => {
                            let context = gw_challenge::context::build_challenge_context(
                                db,
                                target.to_owned(),
                            )?;
                            Ok(SyncEvent::BadBlock { context })
                        }
                        None => Ok(SyncEvent::Success),
                    }
                }
                (Status::Halting, L1ActionContext::Revert { reverted_blocks }) => {
                    let status: u8 = global_state.status().into();
                    assert_eq!(Status::try_from(status), Ok(Status::Running));

                    let first_reverted_block = reverted_blocks.first().expect("first block");
                    let first_reverted_block_number =
                        db.get_block_number(&first_reverted_block.hash().into())?;
                    if first_reverted_block_number.is_none() {
                        return Err(anyhow!("chain fork, can't find first reverted block"));
                    }

                    // Ensure no valid block is reverted
                    if self.challenge_target.is_none() {
                        panic!("a valid block is reverted");
                    }

                    if let Some(block_hash) = self.bad_block_hash() {
                        let local_bad_block = db.get_block(&block_hash)?;
                        let local_bad_block_number =
                            local_bad_block.map(|b| b.raw().number().unpack());

                        assert!(first_reverted_block_number >= local_bad_block_number);
                    }

                    // Both bad blocks and reverted_blocks should be ascended and matched
                    let local_reverted_blocks =
                        package_bad_blocks(db, &first_reverted_block.hash().into())?;
                    let local_slice: Vec<[u8; 32]> =
                        local_reverted_blocks.iter().map(|b| b.hash()).collect();
                    let submit_slice: Vec<[u8; 32]> =
                        reverted_blocks.iter().map(|b| b.hash()).collect();
                    assert_eq!(local_slice, submit_slice);

                    // Revert bad blocks
                    let prev_reverted_block_root = db.get_reverted_block_smt_root()?;
                    db.revert_bad_blocks(&local_reverted_blocks)?;
                    log::debug!("bad blocks reverted");

                    let reverted_block_hashes =
                        local_reverted_blocks.iter().map(|b| b.hash().into());
                    db.set_reverted_block_hashes(
                        &db.get_reverted_block_smt_root()?,
                        prev_reverted_block_root,
                        reverted_block_hashes.collect(),
                    )?;

                    // Check reverted block root
                    let global_reverted_block_root: H256 =
                        global_state.reverted_block_root().unpack();
                    let local_reverted_block_root = db.get_reverted_block_smt_root()?;
                    assert_eq!(local_reverted_block_root, global_reverted_block_root);

                    // Check block smt
                    let global_block_smt = global_state.block();
                    let local_block_smt = {
                        let root: [u8; 32] = db.get_block_smt_root()?.into();
                        BlockMerkleState::new_builder()
                            .merkle_root(root.pack())
                            .count(first_reverted_block.number())
                            .build()
                    };
                    assert_eq!(local_block_smt.as_slice(), global_block_smt.as_slice());

                    // Check db tip block, update local state tip block
                    let parent_block_hash: H256 = first_reverted_block.parent_block_hash().unpack();
                    let global_tip_block_hash: H256 = global_state.tip_block_hash().unpack();
                    assert_eq!(parent_block_hash, global_tip_block_hash);

                    let local_tip_block_hash: H256 = db.get_tip_block_hash()?;
                    assert_eq!(local_tip_block_hash, global_tip_block_hash);

                    let local_tip_block = db.get_tip_block()?;
                    self.local_state.tip = local_tip_block;
                    log::debug!("revert chain local state tip block");

                    let local_tip_block_number = self.local_state.tip.raw().number().unpack();
                    log::info!("revert to block {}", local_tip_block_number);

                    // Check whether our bad block is reverted
                    if Some(H256::from(first_reverted_block.hash())) == self.bad_block_hash() {
                        self.challenge_target = None;
                        log::info!("clear local bad block");
                    }

                    // NOTE: Ensure account smt is valid only when bad block is reverted
                    if self.bad_block_hash().is_none() {
                        let prev_account_smt = first_reverted_block.prev_account();
                        let global_account_smt = global_state.account();
                        assert_eq!(prev_account_smt.as_slice(), global_account_smt.as_slice());
                    }

                    // If our bad block isn't reverted, just challenge it
                    match self.challenge_target {
                        Some(ref target) => {
                            let context = gw_challenge::context::build_challenge_context(
                                db,
                                target.to_owned(),
                            )?;
                            Ok(SyncEvent::BadBlock { context })
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
        log::debug!("last sync event {:?}", self.last_sync_event);

        Ok(())
    }

    pub fn calculate_and_store_finalized_custodians(
        &mut self,
        db: &StoreTransaction,
        block_number: u64,
    ) -> Result<(), anyhow::Error> {
        let block_hash = db
            .get_block_hash_by_number(block_number)?
            .context("get block hash")?;
        let withdrawals = db
            .get_block(&block_hash)?
            .context("get block")?
            .withdrawals();

        let mut finalized_custodians = db
            .get_block_post_finalized_custodian_capacity(block_number - 1)
            .context("get parent block remaining finalized custodians")?
            .as_reader()
            .unpack();
        let last_finalized_block = self
            .generator
            .rollup_context()
            .last_finalized_block_number(block_number - 1);
        let deposits = db
            .get_block_deposit_info_vec(last_finalized_block)
            .context("get last finalized block deposit")?;
        for deposit in deposits {
            let deposit = deposit.request();
            finalized_custodians.capacity = finalized_custodians
                .capacity
                .checked_add(deposit.capacity().unpack().into())
                .context("add capacity overflow")?;
            finalized_custodians
                .checked_add_sudt(
                    deposit.sudt_script_hash().unpack(),
                    deposit.amount().unpack(),
                    deposit.script(),
                )
                .context("add sudt overflow")?;
        }
        for w in withdrawals.as_reader().iter() {
            finalized_custodians.capacity = finalized_custodians
                .capacity
                .checked_sub(w.raw().capacity().unpack().into())
                .context("withdrawal not enough capacity")?;

            let sudt_amount = w.raw().amount().unpack();
            let sudt_script_hash: [u8; 32] = w.raw().sudt_script_hash().unpack();
            if 0 != sudt_amount && CKB_SUDT_SCRIPT_ARGS != sudt_script_hash {
                finalized_custodians
                    .checked_sub_sudt(sudt_script_hash, sudt_amount)
                    .context("withdrawal not enough sudt amount")?;
            }
        }
        db.set_block_post_finalized_custodian_capacity(
            block_number,
            &finalized_custodians.pack().as_reader(),
        )?;
        Ok(())
    }

    /// revert a layer1 action
    pub fn revert_l1action(
        &mut self,
        db: &StoreTransaction,
        action: RevertedL1Action,
    ) -> Result<()> {
        let RevertedL1Action {
            prev_global_state,
            context,
            ..
        } = action;

        let revert = || -> Result<()> {
            match context {
                RevertL1ActionContext::SubmitValidBlock { l2block } => {
                    assert!(
                        self.challenge_target.is_none(),
                        "rewind to last valid tip first"
                    );

                    let local_state_tip_hash: H256 = self.local_state.tip.hash().into();
                    let last_valid_tip_hash = db.get_last_valid_tip_block_hash()?;
                    let block_hash: H256 = l2block.hash().into();
                    assert_eq!(
                        local_state_tip_hash, last_valid_tip_hash,
                        "rewind to last valid tip first"
                    );
                    assert_eq!(
                        block_hash, local_state_tip_hash,
                        "l1 revert must be last valid tip"
                    );

                    let local_state_global_state = &self.local_state.last_global_state;
                    let last_valid_tip_global_state = db
                        .get_block_post_global_state(&block_hash)?
                        .expect("last valid tip global state should exists");
                    assert_eq!(
                        local_state_global_state.as_slice(),
                        last_valid_tip_global_state.as_slice(),
                        "rewind to last valid tip first"
                    );

                    // detach block from DB
                    db.detach_block(&l2block)?;
                    // detach block state from state tree
                    {
                        let mut tree = db.state_tree(StateContext::DetachBlock(
                            l2block.raw().number().unpack(),
                        ))?;
                        tree.detach_block_state()?;
                    }

                    // Check local tip block
                    let local_tip = db.get_tip_block()?;
                    let local_valid_tip = db.get_last_valid_tip_block()?;
                    assert_eq!(local_tip.hash(), local_valid_tip.hash());

                    let parent_block_hash: H256 = l2block.raw().parent_block_hash().unpack();
                    assert_eq!(parent_block_hash, local_tip.hash().into());

                    let l2block_number: u64 = l2block.raw().number().unpack();
                    let local_tip_number: u64 = local_tip.raw().number().unpack();
                    assert_eq!(l2block_number.saturating_sub(1), local_tip_number);

                    // Check reverted block smt
                    let prev_state_reverted_block_root: H256 =
                        prev_global_state.reverted_block_root().unpack();
                    let local_state_reverted_block_root: H256 =
                        local_state_global_state.reverted_block_root().unpack();
                    if local_state_reverted_block_root != prev_state_reverted_block_root {
                        // Rewind reverted block smt
                        let genesis_hash = db.get_block_hash_by_number(0)?.expect("genesis hash");
                        let genesis_reverted_block_root: H256 = {
                            let genesis_global_state = db
                                .get_block_post_global_state(&genesis_hash)?
                                .expect("genesis global state should exists");
                            genesis_global_state.reverted_block_root().unpack()
                        };
                        let mut current_reverted_block_root = local_state_reverted_block_root;
                        while current_reverted_block_root != prev_state_reverted_block_root {
                            if current_reverted_block_root == genesis_reverted_block_root {
                                break;
                            }

                            let reverted_block_hashes = db
                                .get_reverted_block_hashes_by_root(&current_reverted_block_root)?
                                .expect("reverted block hashes should exists")
                                .block_hashes;

                            db.rewind_reverted_block_smt(reverted_block_hashes)?;
                            current_reverted_block_root = db.get_reverted_block_smt_root()?;
                        }
                        assert_eq!(current_reverted_block_root, prev_state_reverted_block_root);
                    }

                    // Check current state
                    let expected_state = l2block.raw().prev_account();
                    let tree = db.state_tree(StateContext::ReadOnly)?;
                    let expected_root: H256 = expected_state.merkle_root().unpack();
                    let expected_count: u32 = expected_state.count().unpack();
                    assert_eq!(tree.calculate_root()?, expected_root);
                    assert_eq!(tree.get_account_count()?, expected_count);

                    // Check genesis state still consistent
                    let script_hash = tree.get_script_hash(0)?;
                    assert!(!script_hash.is_zero());

                    Ok(())
                }
                RevertL1ActionContext::RewindToLastValidTip => {
                    let local_state_tip_hash: H256 = self.local_state.tip.hash().into();
                    let last_valid_tip_block_hash = db.get_last_valid_tip_block_hash()?;

                    let local_state_global_state = &self.local_state.last_global_state;
                    let last_valid_tip_global_state = db
                        .get_block_post_global_state(&last_valid_tip_block_hash)?
                        .expect("last valid tip global state should exists");

                    let local_reverted_block_root: H256 = db.get_reverted_block_smt_root()?;
                    let last_valid_tip_reverted_block_root: H256 =
                        last_valid_tip_global_state.reverted_block_root().unpack();

                    if local_state_tip_hash == last_valid_tip_block_hash
                        && local_state_global_state.as_slice()
                            == last_valid_tip_global_state.as_slice()
                        && local_reverted_block_root == last_valid_tip_reverted_block_root
                    {
                        // No need to rewind
                        return Ok(());
                    }

                    // NOTE: We don't rewind account state, since it's designed to be always
                    // consistent with last valid tip block. Bad block, center challenge,
                    // cancel challenge and revert won't modify it. We will check account state
                    // after sync complete.

                    // Rewind reverted block smt to last valid tip in db
                    let mut current_reverted_block_root = local_reverted_block_root;
                    let genesis_hash = db.get_block_hash_by_number(0)?.expect("genesis hash");
                    let genesis_reverted_block_root: H256 = {
                        let genesis_global_state = db
                            .get_block_post_global_state(&genesis_hash)?
                            .expect("genesis global state should exists");
                        genesis_global_state.reverted_block_root().unpack()
                    };
                    while current_reverted_block_root != last_valid_tip_reverted_block_root {
                        if current_reverted_block_root == genesis_reverted_block_root {
                            break;
                        }

                        let reverted_block_hashes = db
                            .get_reverted_block_hashes_by_root(&current_reverted_block_root)?
                            .expect("reverted block hashes should exists")
                            .block_hashes;

                        db.rewind_reverted_block_smt(reverted_block_hashes)?;
                        current_reverted_block_root = db.get_reverted_block_smt_root()?;
                    }
                    assert_eq!(
                        current_reverted_block_root,
                        last_valid_tip_reverted_block_root
                    );

                    // Rewind block smt to last valid tip in db
                    let mut current_block_hash: H256 =
                        local_state_global_state.tip_block_hash().unpack();
                    while current_block_hash != last_valid_tip_block_hash {
                        if current_block_hash == genesis_hash {
                            break;
                        }

                        let block = db
                            .get_block(&current_block_hash)?
                            .expect("rewind block should exists");

                        db.rewind_block_smt(&block)?;
                        current_block_hash = block.raw().parent_block_hash().unpack();
                    }
                    assert_eq!(current_block_hash, last_valid_tip_block_hash);

                    // Rewind tip block in db
                    db.set_tip_block_hash(last_valid_tip_block_hash)?;

                    Ok(())
                }
            }
        };

        revert()?;
        self.last_sync_event = SyncEvent::Success;
        self.challenge_target = None;

        self.local_state.last_global_state = prev_global_state;
        self.local_state.tip = db.get_tip_block()?;
        Ok(())
    }

    /// Sync chain from layer1
    pub async fn sync(&mut self, param: SyncParam) -> Result<()> {
        let db = self.store.begin_transaction();
        let is_l1_revert_happend = !param.reverts.is_empty();
        // revert layer1 actions
        if !param.reverts.is_empty() {
            // revert
            for reverted_action in param.reverts {
                self.revert_l1action(&db, reverted_action)?;
            }
        }
        let has_bad_block_before_update = self.challenge_target.is_some();

        let updates = param.updates;

        // update layer1 actions
        log::debug!(target: "sync-block", "sync {} actions", updates.len());
        for (i, action) in updates.into_iter().enumerate() {
            let t = Instant::now();
            self.update_l1action(&db, action)?;
            log::debug!(target: "sync-block", "process {}th action cost {}ms", i, t.elapsed().as_millis());
            match self.last_sync_event() {
                SyncEvent::Success => (),
                _ => db.commit()?,
            }
        }

        db.commit()?;

        // Should reset mem pool after bad block is reverted. Deposit cell may pass cancel timeout
        // and get reclaimed. Finalized custodians may be merged in bad block submit tx and this
        // will not be reverted.
        let is_bad_block_reverted = has_bad_block_before_update && self.challenge_target.is_none();
        let tip_block_hash: H256 = self.local_state.tip.hash().into();
        if let Some(mem_pool) = &self.mem_pool {
            if matches!(self.last_sync_event, SyncEvent::Success)
                && (is_l1_revert_happend || is_bad_block_reverted)
            {
                // update mem pool state
                log::debug!(target: "sync-block", "acquire mem-pool",);
                let t = Instant::now();
                // TODO: local cells manager.
                mem_pool
                    .lock()
                    .await
                    .notify_new_tip(tip_block_hash, &Default::default())
                    .await?;
                log::debug!("[sync] unlock mem-pool {}ms", t.elapsed().as_millis());
            }
        }

        // check consistency of account SMT
        let expected_account = match self.challenge_target {
            Some(_) => db.get_last_valid_tip_block()?.raw().post_account(),
            None => self.local_state.tip.raw().post_account(),
        };

        assert_eq!(
            db.account_smt().unwrap().root(),
            &expected_account.merkle_root().unpack(),
            "account root consistent in DB"
        );

        let tree = db.state_tree(StateContext::ReadOnly)?;
        let current_account = tree.merkle_state()?;

        assert_eq!(
            current_account.as_slice(),
            expected_account.as_slice(),
            "check account tree"
        );

        log::debug!(target: "sync-block", "Complete");
        Ok(())
    }

    /// Only for testing.
    pub async fn notify_new_tip(&self) -> Result<()> {
        if let Some(mem_pool) = &self.mem_pool {
            let tip_block_hash = self.store.get_last_valid_tip_block_hash().unwrap();
            mem_pool
                .lock()
                .await
                .notify_new_tip(tip_block_hash, &Default::default())
                .await?;
        }
        Ok(())
    }

    /// Store a new local block.
    ///
    /// Note that this does not store finalized custodians.
    #[instrument(skip_all)]
    pub fn update_local(
        &mut self,
        store_tx: &StoreTransaction,
        l2_block: L2Block,
        deposit_info_vec: DepositInfoVec,
        deposit_asset_scripts: HashSet<Script>,
        withdrawals: Vec<WithdrawalRequestExtra>,
        global_state: GlobalState,
    ) -> Result<()> {
        let local_tip = self.local_state.tip();
        let parent_block_hash: [u8; 32] = l2_block.raw().parent_block_hash().unpack();
        if parent_block_hash != local_tip.hash() {
            bail!("fork detected");
        }

        // Reverted block root should not change
        let local_reverted_block_root = store_tx.get_reverted_block_smt_root()?;
        let global_reverted_block_root: H256 = global_state.reverted_block_root().unpack();
        assert_eq!(local_reverted_block_root, global_reverted_block_root);

        // TODO??: check bad block challenge target.
        let maybe_challenge_target = self.process_block(
            store_tx,
            l2_block,
            global_state,
            deposit_info_vec,
            deposit_asset_scripts,
            withdrawals,
        )?;

        if let Some(challenge_target) = maybe_challenge_target {
            bail!(
                "process_block returned challenge target: {}",
                challenge_target
            );
        }

        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn process_block(
        &mut self,
        db: &StoreTransaction,
        l2block: L2Block,
        global_state: GlobalState,
        deposit_info_vec: DepositInfoVec,
        deposit_asset_scripts: HashSet<Script>,
        withdrawals: Vec<WithdrawalRequestExtra>,
    ) -> Result<Option<ChallengeTarget>> {
        let tip_number: u64 = self.local_state.tip.raw().number().unpack();
        let tip_block_hash = self.local_state.tip.raw().hash();
        let block_number: u64 = l2block.raw().number().unpack();
        assert_eq!(
            {
                let parent_block_hash = l2block.raw().parent_block_hash();
                (block_number, parent_block_hash)
            },
            {
                let tip_block_hash: Byte32 = tip_block_hash.pack();
                (tip_number + 1, tip_block_hash)
            },
            "new l2block must be the successor of the tip"
        );

        // process l2block
        let args = ApplyBlockArgs {
            l2block: l2block.clone(),
            deposit_info_vec: deposit_info_vec.clone(),
            withdrawals: withdrawals.clone(),
        };
        let tip_block_hash = self.local_state.tip().hash().into();
        let chain_view = ChainView::new(db, tip_block_hash);

        {
            let tree = db.state_tree(StateContext::ReadOnly)?;

            let prev_merkle_state = l2block.raw().prev_account();
            assert_eq!(
                tree.merkle_state()?.as_slice(),
                prev_merkle_state.as_slice(),
                "prev account merkle state must be consistent"
            );
        }

        // process transactions
        // TODO: run offchain validator before send challenge, to make sure the block is bad
        let generator = &self.generator;
        let (withdrawal_receipts, prev_txs_state, tx_receipts) = match generator
            .verify_and_apply_block(db, &chain_view, args, &self.skipped_invalid_block_list)
        {
            ApplyBlockResult::Success {
                tx_receipts,
                prev_txs_state,
                withdrawal_receipts,
                offchain_used_cycles,
            } => {
                log::debug!(
                    "Process #{} txs: {} offchain used cycles {}",
                    block_number,
                    tx_receipts.len(),
                    offchain_used_cycles
                );
                (withdrawal_receipts, prev_txs_state, tx_receipts)
            }
            ApplyBlockResult::Challenge { target, error } => {
                log::warn!("verify #{} state transition error {}", block_number, error);
                return Ok(Some(target));
            }
            ApplyBlockResult::Error(err) => return Err(err.into()),
        };

        // update chain
        let deposit_info_vec_len = deposit_info_vec.len();
        let withdrawals_len = withdrawals.len();
        let tx_receipts_len = tx_receipts.len();
        db.insert_block(
            l2block.clone(),
            global_state.clone(),
            withdrawal_receipts,
            prev_txs_state,
            tx_receipts,
            deposit_info_vec,
            withdrawals,
        )?;
        db.insert_asset_scripts(deposit_asset_scripts)?;
        db.attach_block(l2block.clone())?;

        // Update metrics.
        gw_metrics::BLOCK_HEIGHT.set(l2block.raw().number().unpack());
        gw_metrics::DEPOSITS.inc_by(deposit_info_vec_len as u64);
        gw_metrics::WITHDRAWALS.inc_by(withdrawals_len as u64);
        gw_metrics::TRANSACTIONS.inc_by(tx_receipts_len as u64);

        self.local_state.tip = l2block;
        self.local_state.last_global_state = global_state;
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

    global_state_from_slice(&output_data).map_err(|_| anyhow!("global state unpacking error"))
}

fn package_bad_blocks(db: &StoreTransaction, start_block_hash: &H256) -> Result<Vec<L2Block>> {
    let tip_block = db.get_tip_block()?;
    if tip_block.hash() == start_block_hash.as_slice() {
        return Ok(vec![tip_block]);
    }

    let tip_block_number = tip_block.raw().number().unpack();
    let start_block_number = {
        let number = db.get_block_number(start_block_hash)?;
        number.ok_or_else(|| anyhow!("challenge block number not found"))?
    };
    assert!(start_block_number < tip_block_number);

    let to_block = |number: u64| {
        let hash = db.get_block_hash_by_number(number)?;
        let block = hash.map(|h| db.get_block(&h)).transpose()?.flatten();
        block.ok_or_else(|| anyhow!("block {} not found", number))
    };

    (start_block_number..=tip_block_number)
        .map(to_block)
        .collect()
}
