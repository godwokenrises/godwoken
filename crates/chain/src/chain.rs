use anyhow::{anyhow, Result};
use gw_common::{sparse_merkle_tree, state::State, H256};
use gw_config::{ChainConfig, GenesisConfig};
use gw_generator::{
    generator::StateTransitionArgs, ChallengeContext, Error as GeneratorError, Generator,
};
use gw_mem_pool::pool::MemPool;
use gw_store::{transaction::StoreTransaction, Store};
use gw_traits::ChainStore;
use gw_types::{
    bytes::Bytes,
    core::Status,
    packed::{
        ChallengeTarget, ChallengeWitness, DepositionRequest, GlobalState, HeaderInfo, L2Block,
        L2BlockReader, RollupConfig, Transaction, TxReceipt, VerifyTransactionWitness, WitnessArgs,
        WitnessArgsReader,
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
    /// transactions' header info
    pub header_info: HeaderInfo,
    pub context: L1ActionContext,
}

#[derive(Debug, Eq, PartialEq, Clone)]
pub struct RevertedL1Action {
    /// input global state
    pub prev_global_state: GlobalState,
    /// transaction
    pub transaction: Transaction,
    /// transactions' header info
    pub header_info: HeaderInfo,
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
    last_synced: HeaderInfo,
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

    pub fn last_synced(&self) -> &HeaderInfo {
        &self.last_synced
    }

    pub fn last_global_state(&self) -> &GlobalState {
        &self.last_global_state
    }
}

pub struct Chain {
    pub rollup_type_script_hash: [u8; 32],
    pub rollup_config_hash: [u8; 32],
    pub rollup_config: RollupConfig,
    pub store: Store,
    pub bad_block_context: Option<ChallengeTarget>,
    pub local_state: LocalState,
    pub generator: Arc<Generator>,
    pub mem_pool: Arc<Mutex<MemPool>>,
}

impl Chain {
    pub fn create(
        config: ChainConfig,
        store: Store,
        generator: Arc<Generator>,
        mem_pool: Arc<Mutex<MemPool>>,
    ) -> Result<Self> {
        let ChainConfig {
            rollup_type_script,
            rollup_config,
        } = config;
        let rollup_type_script_hash = rollup_type_script.hash();
        let chain_id: [u8; 32] = store.get_chain_id()?.into();
        assert_eq!(
            chain_id, rollup_type_script_hash,
            "Database chain_id must equals to rollup_script_hash"
        );
        let tip = store.get_tip_block()?;
        let last_synced = store
            .get_block_synced_header_info(&tip.hash().into())?
            .ok_or(anyhow!("can't find last synced header info"))?;
        let last_global_state = store
            .get_block_post_global_state(&tip.hash().into())?
            .ok_or(anyhow!("can't find last global state"))?;
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
            rollup_config,
        })
    }

    /// return local state
    pub fn local_state(&self) -> &LocalState {
        &self.local_state
    }

    pub fn store(&self) -> &Store {
        &self.store
    }

    pub fn rollup_config(&self) -> &RollupConfig {
        &self.rollup_config
    }

    pub fn rollup_config_hash(&self) -> &[u8; 32] {
        &self.rollup_config_hash
    }

    /// update a layer1 action
    fn update_l1action(&mut self, db: &StoreTransaction, action: L1Action) -> Result<SyncEvent> {
        let L1Action {
            transaction,
            header_info,
            context,
        } = action;
        let global_state = parse_global_state(&transaction, &self.rollup_type_script_hash)?;
        assert!(
            {
                let number: u64 = header_info.number().unpack();
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
                if let Some(challenge_context) = self.process_block(
                    db,
                    l2block.clone(),
                    header_info.clone(),
                    global_state.clone(),
                    deposition_requests,
                )? {
                    // stop syncing and return event
                    self.bad_block_context = Some(challenge_context.target.clone());
                    SyncEvent::BadBlock(challenge_context)
                } else {
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
                    let _tx_receipt = unimplemented!();
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
        self.local_state.last_global_state = global_state.clone();
        self.local_state.last_synced = header_info;
        Ok(event)
    }

    /// revert a layer1 action
    fn revert_l1action(&mut self, db: &StoreTransaction, action: RevertedL1Action) -> Result<()> {
        let RevertedL1Action {
            prev_global_state,
            transaction,
            header_info,
            context,
        } = action;
        assert!(
            {
                let number: u64 = header_info.number().unpack();
                number
            } <= {
                let number: u64 = self.local_state.last_synced.number().unpack();
                number
            },
            "must be smaller than or equalled to last synced number"
        );
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
            }
            _ => {
                // do nothing
            }
        };

        // update last global state
        self.local_state.last_global_state = prev_global_state.clone();
        self.local_state.tip = db.get_tip_block()?;
        self.local_state.last_synced = db
            .get_block_synced_header_info(&self.local_state.tip.hash().into())?
            .expect("last header info");
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
            // reconstruct account state tree
            let event = self.replay_chain(&db)?;
            if event != SyncEvent::Success {
                db.commit()?;
                return Ok(event);
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
            assert_eq!(
                db.get_account_smt_root().unwrap(),
                expected_account_root,
                "account root consistent in DB"
            );
            let tree = db.account_state_tree().unwrap();
            let current_account_root = tree.calculate_root().unwrap();
            assert_eq!(
                current_account_root, expected_account_root,
                "check account tree"
            );
        }
        Ok(SyncEvent::Success)
    }

    // replay chain to reconstruct account SMT
    // TODO this method should be replaced with a version based storage
    fn replay_chain(&mut self, db: &StoreTransaction) -> Result<SyncEvent> {
        let tip_number: u64 = self.local_state.tip.raw().number().unpack();
        // reset local state
        let genesis_hash = db.get_block_hash_by_number(0)?.expect("genesis").into();
        let genesis = db.get_block(&genesis_hash)?.expect("genesis");
        let genesis_header_info = db
            .get_block_synced_header_info(&genesis_hash.into())?
            .expect("genesis");
        let genesis_global_state = db
            .get_block_post_global_state(&genesis_hash.into())?
            .expect("genesis");
        self.local_state = LocalState {
            tip: genesis.clone(),
            last_synced: genesis_header_info,
            last_global_state: genesis_global_state,
        };
        // reset account SMT to genesis
        // TODO use version based storage
        db.clear_account_state_tree()?;
        gw_generator::genesis::build_genesis_from_store(
            db,
            &GenesisConfig {
                timestamp: genesis.raw().timestamp().unpack(),
            },
            self.rollup_config(),
        )?;
        // replay blocks
        for number in 1..tip_number {
            let block_hash = db
                .get_block_hash_by_number(number)?
                .expect("get l2block")
                .into();
            let l2block = db.get_block(&block_hash)?.expect("l2block");
            let header_info = db
                .get_block_synced_header_info(&block_hash)?
                .expect("get l2block header info");
            let global_state = db
                .get_block_post_global_state(&block_hash)?
                .expect("get l2block global state");
            let deposition_requests = db
                .get_block_deposition_requests(&block_hash)?
                .expect("get l2block deposition requests");
            if let Some(challenge_context) = self.process_block(
                db,
                l2block.clone(),
                header_info.clone(),
                global_state.clone(),
                deposition_requests,
            )? {
                // stop syncing and return event
                self.bad_block_context = Some(challenge_context.target.clone());
                return Ok(SyncEvent::BadBlock(challenge_context));
            }
        }
        Ok(SyncEvent::Success)
    }

    fn process_block(
        &mut self,
        db: &StoreTransaction,
        l2block: L2Block,
        header_info: HeaderInfo,
        global_state: GlobalState,
        deposition_requests: Vec<DepositionRequest>,
    ) -> Result<Option<ChallengeContext>> {
        let tip_number: u64 = self.local_state.tip.raw().number().unpack();
        let tip_block_hash = self.local_state.tip.raw().hash();
        assert_eq!(
            {
                let number: u64 = l2block.raw().number().unpack();
                let parent_block_hash: [u8; 32] = l2block.raw().parent_block_hash().unpack();
                (number, parent_block_hash)
            },
            (tip_number + 1, tip_block_hash),
            "new l2block must be the successor of the tip"
        );

        // process l2block
        let args = StateTransitionArgs {
            l2block: l2block.clone(),
            deposition_requests: deposition_requests.clone(),
        };
        let mut tree = db.account_state_tree()?;
        // process transactions
        let result = match self.generator.apply_state_transition(db, &mut tree, args) {
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
            header_info,
            global_state,
            result.receipts,
            deposition_requests,
        )?;
        db.attach_block(l2block.clone())?;
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
