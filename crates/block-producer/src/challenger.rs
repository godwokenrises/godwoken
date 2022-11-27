#![allow(clippy::mutable_key_type)]

use crate::cleaner::{Cleaner, Verifier};
use crate::test_mode_control::TestModeControl;
use crate::types::ChainEvent;
use crate::utils;
use anyhow::{anyhow, bail, Context, Result};
use ckb_types::prelude::{Builder, Entity, Reader};
use gw_chain::chain::{Chain, ChallengeCell, SyncEvent};
use gw_challenge::cancel_challenge::{
    CancelChallengeOutput, LoadData, LoadDataContext, LoadDataStrategy, RecoverAccounts,
    RecoverAccountsContext,
};
use gw_challenge::enter_challenge::EnterChallenge;
use gw_challenge::offchain::verify_tx::{verify_tx, TxWithContext};
use gw_challenge::offchain::{mock_cancel_challenge_tx, OffChainMockContext};
use gw_challenge::revert::Revert;
use gw_challenge::types::{RevertContext, VerifyContext};
use gw_common::H256;
use gw_config::{BlockProducerConfig, DebugConfig};
use gw_generator::types::vm::ChallengeContext;
use gw_jsonrpc_types::test_mode::TestModePayload;
use gw_rpc_client::contract::ContractsCellDepManager;
use gw_rpc_client::rpc_client::RPCClient;
use gw_types::bytes::Bytes;
use gw_types::core::{ChallengeTargetType, Status};
use gw_types::offchain::{global_state_from_slice, CellInfo, InputCellInfo, TxStatus};
use gw_types::packed::{
    CellDep, CellInput, CellOutput, ChallengeLockArgs, ChallengeLockArgsReader, ChallengeTarget,
    GlobalState, OutPoint, Script, Transaction, WitnessArgs,
};
use gw_types::prelude::{Pack, Unpack};
use gw_utils::fee::fill_tx_fee;
use gw_utils::genesis_info::CKBGenesisInfo;
use gw_utils::transaction_skeleton::TransactionSkeleton;
use gw_utils::wallet::Wallet;
use gw_utils::RollupContext;
use tokio::sync::Mutex;
use tracing::instrument;

use std::collections::{HashMap, HashSet};
use std::convert::TryFrom;
use std::sync::Arc;
use std::time::Duration;

const MAX_CANCEL_CYCLES: u64 = 7000_0000;
const MAX_CANCEL_TX_BYTES: u64 = ckb_chain_spec::consensus::MAX_BLOCK_BYTES;
const TRANSACTION_FAILED_TO_RESOLVE_ERROR: &str = "TransactionFailedToResolve";

pub struct Challenger {
    rollup_context: RollupContext,
    rpc_client: RPCClient,
    wallet: Wallet,
    config: BlockProducerConfig,
    ckb_genesis_info: CKBGenesisInfo,
    builtin_load_data: HashMap<H256, CellDep>,
    chain: Arc<Mutex<Chain>>,
    tests_control: Option<TestModeControl>,
    cleaner: Arc<Cleaner>,
    debug_config: DebugConfig,
    offchain_mock_context: OffChainMockContext,
    contracts_dep_manager: ContractsCellDepManager,
    last_submit_tx: Option<H256>,
}

pub struct ChallengerNewArgs {
    pub rollup_context: RollupContext,
    pub rpc_client: RPCClient,
    pub wallet: Wallet,
    pub config: BlockProducerConfig,
    pub debug_config: DebugConfig,
    pub builtin_load_data: HashMap<H256, CellDep>,
    pub ckb_genesis_info: CKBGenesisInfo,
    pub chain: Arc<Mutex<Chain>>,
    pub tests_control: Option<TestModeControl>,
    pub cleaner: Arc<Cleaner>,
    pub offchain_mock_context: OffChainMockContext,
    pub contracts_dep_manager: ContractsCellDepManager,
}

impl Challenger {
    pub fn new(args: ChallengerNewArgs) -> Self {
        let ChallengerNewArgs {
            rollup_context,
            rpc_client,
            wallet,
            config,
            debug_config,
            builtin_load_data,
            ckb_genesis_info,
            chain,
            tests_control,
            cleaner,
            offchain_mock_context,
            contracts_dep_manager,
        } = args;

        Self {
            rollup_context,
            rpc_client,
            wallet,
            config,
            debug_config,
            ckb_genesis_info,
            builtin_load_data,
            chain,
            tests_control,
            cleaner,
            offchain_mock_context,
            contracts_dep_manager,
            last_submit_tx: None,
        }
    }

    #[instrument(skip_all, name = "challenger handle_event")]
    pub async fn handle_event(&mut self, event: ChainEvent) -> Result<()> {
        if let Some(ref tests_control) = self.tests_control {
            match tests_control.payload().await {
                Some(TestModePayload::Challenge { .. })
                    | Some(TestModePayload::WaitForChallengeMaturity)
                    | Some(TestModePayload::None) => (),
                    Some(TestModePayload::BadBlock { .. }) // Payload not match (BadBlock for block producer)
                        | None => return Ok(()), // Wait payload
            }
        }

        if let Some(last_submit_tx) = self.last_submit_tx {
            let ckb_client = &self.rpc_client.ckb;
            let tx_status = ckb_client.get_transaction_status(last_submit_tx).await?;
            match tx_status {
                Some(TxStatus::Pending) | Some(TxStatus::Proposed) => return Ok(()),
                _ => {
                    log::debug!("last challenger submit tx {:?}", tx_status);
                    self.last_submit_tx = None;
                }
            }
        }

        let rollup = RollupState::query(&self.rpc_client).await?;
        if let Some(ref tests_control) = self.tests_control {
            if let Some(TestModePayload::Challenge { .. }) = tests_control.payload().await {
                match rollup.status()? {
                    Status::Halting => return Ok(()), // Already halting, do nothing, we can't challenge block when rollup is halted
                    Status::Running => {
                        let context = tests_control.challenge().await?;
                        return self.challenge_block(rollup, context).await;
                    }
                };
            }
        }

        let last_sync_event = { self.chain.lock().await.last_sync_event().to_owned() };
        log::debug!("load chain last sync event {:?}", last_sync_event);

        match last_sync_event {
            SyncEvent::Success => Ok(()),
            SyncEvent::BadBlock { context } => {
                if let Some(ref tests_control) = self.tests_control {
                    match tests_control.payload().await {
                        Some(TestModePayload::WaitForChallengeMaturity) => return Ok(()), // do nothing
                        Some(TestModePayload::None) => tests_control.clear_none().await?,
                        _ => unreachable!(),
                    }
                }
                {
                    let hash = hex::encode::<[u8; 32]>(context.target.block_hash().unpack());
                    let idx: u32 = context.target.target_index().unpack();
                    let type_ = ChallengeTargetType::try_from(context.target.target_type())
                        .map_err(|_| anyhow!("invalid challenge type"))?;
                    log::info!("challenge block 0x{} target {} type {:?}", hash, idx, type_);
                }
                self.challenge_block(rollup, context).await
            }
            SyncEvent::BadChallenge { cell, context } => {
                if let Some(ref tests_control) = self.tests_control {
                    match tests_control.payload().await {
                        Some(TestModePayload::WaitForChallengeMaturity) => return Ok(()), // do nothing
                        Some(TestModePayload::None) => tests_control.clear_none().await?,
                        _ => unreachable!(),
                    }
                }
                self.cancel_challenge(rollup, cell, *context).await
            }
            SyncEvent::WaitChallenge { cell, context } => {
                if let Some(ref tests_control) = self.tests_control {
                    match tests_control.payload().await {
                        Some(TestModePayload::WaitForChallengeMaturity) => {
                            tests_control
                                .wait_for_challenge_maturity(rollup.status()?)
                                .await?
                        }
                        Some(TestModePayload::None) => tests_control.clear_none().await?,
                        _ => unreachable!(),
                    }
                }
                let tip_number = to_tip_number(&event);
                self.revert(rollup, cell, context, tip_number).await
            }
        }
    }

    async fn challenge_block(
        &mut self,
        rollup_state: RollupState,
        context: ChallengeContext,
    ) -> Result<()> {
        if Status::Halting == rollup_state.status()? {
            // Already entered challenge
            return Ok(());
        }

        let block_numer = context.witness.raw_l2block().number().unpack();
        let rewards_lock = {
            let challenger_config = &self.config.challenger_config;
            challenger_config.rewards_receiver_lock.clone().into()
        };
        let prev_state = rollup_state.get_state().to_owned();
        let enter_challenge =
            EnterChallenge::new(prev_state, &self.rollup_context, context, rewards_lock);
        let challenge_output = enter_challenge.build_output();

        // Build challenge transaction
        let omni_lock_code_hash = self.contracts_dep_manager.load_scripts().omni_lock.hash();
        let mut tx_skeleton = TransactionSkeleton::new(omni_lock_code_hash.0);
        let contracts_dep = self.contracts_dep_manager.load();

        // Rollup
        let rollup_deps = vec![
            contracts_dep.rollup_cell_type.clone().into(),
            self.config.rollup_config_cell_dep.clone().into(),
            contracts_dep.omni_lock.clone().into(),
        ];
        let rollup_output = (
            rollup_state.rollup_output(),
            challenge_output.post_global_state.as_bytes(),
        );
        let rollup_witness = challenge_output.rollup_witness;

        tx_skeleton.cell_deps_mut().extend(rollup_deps);
        tx_skeleton.inputs_mut().push(rollup_state.rollup_input());
        tx_skeleton.outputs_mut().push(rollup_output);
        tx_skeleton.witnesses_mut().push(rollup_witness);

        // Challenge
        let challenge_cell = challenge_output.challenge_cell;
        tx_skeleton.outputs_mut().push(challenge_cell);

        let challenger_lock_dep = self.ckb_genesis_info.sighash_dep();
        let challenger_lock = self.wallet.lock_script().to_owned();
        tx_skeleton.cell_deps_mut().push(challenger_lock_dep);
        fill_tx_fee(
            &mut tx_skeleton,
            &self.rpc_client.indexer,
            challenger_lock,
            self.config.fee_rate,
        )
        .await?;

        let tx = self.wallet.sign_tx_skeleton(tx_skeleton)?;

        if let Err(err) = self.dry_run_transaction(&tx, "challenge block").await {
            utils::dump_transaction(&self.debug_config.debug_tx_dump_path, &self.rpc_client, &tx)
                .await;
            bail!(err);
        }

        let tx_hash = self.rpc_client.send_transaction(&tx).await?;
        log::info!("Challenge block {} in tx {}", block_numer, to_hex(&tx_hash));
        self.last_submit_tx = Some(tx_hash);

        Ok(())
    }

    async fn cancel_challenge(
        &mut self,
        rollup_state: RollupState,
        challenge_cell: ChallengeCell,
        context: VerifyContext,
    ) -> Result<()> {
        if Status::Running == rollup_state.status()? {
            // Already cancelled
            return Ok(());
        }

        let prev_state = rollup_state.get_state().to_owned();
        let load_data_strategy = validate_load_data_strategy_offchain(
            &self.offchain_mock_context,
            prev_state.clone(),
            &challenge_cell,
            context.clone(),
        )?;

        let challenge_cell = to_cell_info(challenge_cell);
        let burn_lock = self.config.challenger_config.burn_lock.clone().into();
        let owner_lock = self.wallet.lock_script().to_owned();
        let mut cancel_output = gw_challenge::cancel_challenge::build_output(
            &self.rollup_context,
            prev_state,
            &challenge_cell,
            burn_lock,
            owner_lock,
            context,
            &self.builtin_load_data,
            Some(load_data_strategy),
        )?;

        // Build verifier transaction
        let verifier_cell = cancel_output.verifier_cell.clone();
        let tx = {
            let (load_data, recover_accounts) = (
                cancel_output.load_data.clone(),
                cancel_output.recover_accounts.clone(),
            );
            self.build_verifier_tx(verifier_cell, load_data, recover_accounts)
                .await?
        };
        let verifier_spent_inputs = extract_inputs(&tx);
        let verifier_tx_hash = self.rpc_client.send_transaction(&tx).await?;
        log::info!("Create verifier in tx {}", to_hex(&verifier_tx_hash));

        tokio::time::timeout(
            Duration::from_secs(30),
            self.rpc_client.ckb.wait_tx_proposed(verifier_tx_hash),
        )
        .await
        .with_context(|| format!("waiting for tx proposed 0x{}", to_hex(&verifier_tx_hash)))??;

        // Build cancellation transaction
        let challenge_input = to_input_cell_info(challenge_cell);
        let verifier_context = {
            let contracts_dep = self.contracts_dep_manager.load();
            let cell_dep = cancel_output.verifier_dep(&contracts_dep)?;
            let input = cancel_output.verifier_input(verifier_tx_hash, 0);
            let witness = cancel_output.verifier_witness.clone();
            let load_data = cancel_output.load_data.take();
            let load_data_len = load_data.as_ref().map(|l| l.cell_len()).unwrap_or(0);
            let load_data_context = load_data.map(|ld| ld.into_context(verifier_tx_hash, 0));
            let recover_accounts = cancel_output.recover_accounts.take();
            let recover_accounts_context = recover_accounts
                .map(|ra| ra.into_context(verifier_tx_hash, load_data_len + 1, &contracts_dep))
                .transpose()?;

            VerifierContext::new(
                cell_dep,
                input,
                witness,
                load_data_context,
                recover_accounts_context,
                Some(verifier_spent_inputs),
            )
        };

        let tx = self
            .build_cancel_tx(
                rollup_state,
                cancel_output,
                challenge_input,
                verifier_context.clone(),
            )
            .await?;

        if let Err(err) = self.dry_run_transaction(&tx, "cancel challenge").await {
            utils::dump_transaction(&self.debug_config.debug_tx_dump_path, &self.rpc_client, &tx)
                .await;
            bail!(err);
        }

        let load_data_inputs = verifier_context.load_data_context.map(|d| d.inputs);
        let verifier = Verifier::new(
            load_data_inputs.unwrap_or_default(),
            verifier_context.recover_accounts_context,
            verifier_context.cell_dep,
            verifier_context.input,
            verifier_context.witness,
        );
        match self.rpc_client.send_transaction(&tx).await {
            Ok(tx_hash) => {
                self.cleaner.watch_verifier(verifier, Some(tx_hash)).await;
                log::info!("Cancel challenge in tx {}", to_hex(&tx_hash));
                self.last_submit_tx = Some(tx_hash);
            }
            Err(err) => {
                self.cleaner.watch_verifier(verifier, None).await;
                log::warn!("Cancel challenge failed {}", err);
            }
        }

        Ok(())
    }

    async fn revert(
        &mut self,
        rollup_state: RollupState,
        challenge_cell: ChallengeCell,
        context: RevertContext,
        tip_block_number: u64,
    ) -> Result<()> {
        if Status::Running == rollup_state.status()? {
            // Already reverted
            return Ok(());
        }

        // Check challenge maturity
        let challenge_maturity_blocks: u64 = {
            let config = &self.rollup_context.rollup_config;
            config.challenge_maturity_blocks().unpack()
        };
        let challenge_cell = to_cell_info(challenge_cell);
        let challenge_tx_block_number = {
            let tx_hash: H256 = challenge_cell.out_point.tx_hash().unpack();
            let tx_status = self.rpc_client.ckb.get_transaction_status(tx_hash).await?;
            if !matches!(tx_status, Some(TxStatus::Committed)) {
                log::debug!("challenge tx isn't committed");
                return Ok(());
            }

            let query = self
                .rpc_client
                .ckb
                .get_transaction_block_number(tx_hash)
                .await;
            query?.ok_or_else(|| anyhow!("challenge tx block number not found"))?
        };

        const FLAG_SINCE_RELATIVE: u64 =
            0b1000_0000_0000_0000_0000_0000_0000_0000_0000_0000_0000_0000_0000_0000_0000_0000;
        const FLAG_SINCE_BLOCK_NUMBER: u64 =
            0b000_0000_0000_0000_0000_0000_0000_0000_0000_0000_0000_0000_0000_0000_0000_0000;
        let since = {
            let block_number = ckb_types::core::BlockNumber::from_le_bytes(
                challenge_maturity_blocks.to_le_bytes(),
            );
            FLAG_SINCE_RELATIVE | FLAG_SINCE_BLOCK_NUMBER | block_number
        };

        if tip_block_number.saturating_sub(challenge_tx_block_number) < challenge_maturity_blocks {
            return Ok(());
        }

        // Collect stake cells
        let stake_owner_lock_hashes = {
            let blocks = context.revert_witness.reverted_blocks.clone().into_iter();
            blocks.map(|b| b.stake_cell_owner_lock_hash().unpack())
        };
        let stake_cells = {
            let rpc_client = &self.rpc_client;
            let query = rpc_client.query_stake_cells_by_owner_lock_hashes(stake_owner_lock_hashes);
            query.await?
        };
        let prev_state = rollup_state.get_state().to_owned();
        let burn_lock = self.config.challenger_config.burn_lock.clone().into();

        let revert = Revert::new(
            &self.rollup_context,
            prev_state,
            &challenge_cell,
            &stake_cells,
            burn_lock,
            context,
        );
        let revert_output = revert.build_output()?;

        // Build revert transaction
        let omni_lock_code_hash = self.contracts_dep_manager.load_scripts().omni_lock.hash();
        let mut tx_skeleton = TransactionSkeleton::new(omni_lock_code_hash.0);
        let contracts_dep = self.contracts_dep_manager.load();

        // Rollup
        let rollup_deps = vec![
            contracts_dep.rollup_cell_type.clone().into(),
            self.config.rollup_config_cell_dep.clone().into(),
            contracts_dep.omni_lock.clone().into(),
        ];
        let rollup_output = (
            rollup_state.rollup_output(),
            revert_output.post_global_state.as_bytes(),
        );
        let rollup_witness = revert_output.rollup_witness;

        tx_skeleton.cell_deps_mut().extend(rollup_deps);
        tx_skeleton.inputs_mut().push(rollup_state.rollup_input());
        tx_skeleton.outputs_mut().push(rollup_output);
        tx_skeleton.witnesses_mut().push(rollup_witness);

        // Challenge
        let challenge_input = to_input_cell_info_with_since(challenge_cell, since);
        let challenge_dep = contracts_dep.challenge_cell_lock.clone().into();
        tx_skeleton.cell_deps_mut().push(challenge_dep);
        tx_skeleton.inputs_mut().push(challenge_input);

        // Stake
        let stake_inputs = stake_cells.into_iter().map(to_input_cell_info);
        let stake_dep = contracts_dep.stake_cell_lock.clone().into();
        tx_skeleton.cell_deps_mut().push(stake_dep);
        tx_skeleton.inputs_mut().extend(stake_inputs);

        // Rewards
        tx_skeleton.outputs_mut().extend(revert_output.reward_cells);

        // Burn
        tx_skeleton.outputs_mut().extend(revert_output.burn_cells);

        let challenger_lock_dep = self.ckb_genesis_info.sighash_dep();
        let challenger_lock = self.wallet.lock_script().to_owned();
        tx_skeleton.cell_deps_mut().push(challenger_lock_dep);
        fill_tx_fee(
            &mut tx_skeleton,
            &self.rpc_client.indexer,
            challenger_lock,
            self.config.fee_rate,
        )
        .await?;

        let tx = self.wallet.sign_tx_skeleton(tx_skeleton)?;

        if let Err(err) = self.dry_run_transaction(&tx, "revert block").await {
            utils::dump_transaction(&self.debug_config.debug_tx_dump_path, &self.rpc_client, &tx)
                .await;
            bail!(err);
        }

        let tx_hash = self.rpc_client.send_transaction(&tx).await?;
        log::info!("Revert block in tx {}", to_hex(&tx_hash));
        self.last_submit_tx = Some(tx_hash);

        Ok(())
    }

    async fn build_verifier_tx(
        &self,
        verifier: (CellOutput, Bytes),
        load_data: Option<LoadData>,
        recover_accounts: Option<RecoverAccounts>,
    ) -> Result<Transaction> {
        let mut tx_skeleton = TransactionSkeleton::default();
        tx_skeleton.outputs_mut().push(verifier);

        if let Some(load_data) = load_data {
            tx_skeleton.outputs_mut().extend(load_data.cells);
        }

        if let Some(recover_accounts) = recover_accounts {
            tx_skeleton.outputs_mut().extend(recover_accounts.cells);
        }

        let challenger_lock_dep = self.ckb_genesis_info.sighash_dep();
        let challenger_lock = self.wallet.lock_script().to_owned();
        tx_skeleton.cell_deps_mut().push(challenger_lock_dep);
        fill_tx_fee(
            &mut tx_skeleton,
            &self.rpc_client.indexer,
            challenger_lock,
            self.config.fee_rate,
        )
        .await?;

        self.wallet.sign_tx_skeleton(tx_skeleton)
    }

    async fn build_cancel_tx(
        &self,
        rollup_state: RollupState,
        cancel_output: CancelChallengeOutput,
        challenge_input: InputCellInfo,
        verifier_context: VerifierContext,
    ) -> Result<Transaction> {
        let omni_lock_code_hash = self.contracts_dep_manager.load_scripts().omni_lock.hash();
        let mut tx_skeleton = TransactionSkeleton::new(omni_lock_code_hash.0);
        let contracts_dep = self.contracts_dep_manager.load();

        // Rollup
        let rollup_deps = vec![
            contracts_dep.rollup_cell_type.clone().into(),
            self.config.rollup_config_cell_dep.clone().into(),
            contracts_dep.omni_lock.clone().into(),
        ];
        let rollup_output = (
            rollup_state.rollup_output(),
            cancel_output.post_global_state.as_bytes(),
        );
        let rollup_witness = cancel_output.rollup_witness;

        tx_skeleton.cell_deps_mut().extend(rollup_deps);
        tx_skeleton.inputs_mut().push(rollup_state.rollup_input());
        tx_skeleton.outputs_mut().push(rollup_output);
        tx_skeleton.witnesses_mut().push(rollup_witness);

        // Challenge
        let challenge_dep = contracts_dep.challenge_cell_lock.clone().into();
        let challenge_witness = cancel_output.challenge_witness;
        tx_skeleton.cell_deps_mut().push(challenge_dep);
        tx_skeleton.inputs_mut().push(challenge_input);
        tx_skeleton.witnesses_mut().push(challenge_witness);

        // Verifier
        let verifier_tx_hash = verifier_context.tx_hash();
        tx_skeleton.cell_deps_mut().push(verifier_context.cell_dep);
        if let Some(load_data_context) = verifier_context.load_data_context {
            let builtin_cell_deps = load_data_context.builtin_cell_deps;
            let cell_deps = load_data_context.cell_deps;
            tx_skeleton.cell_deps_mut().extend(builtin_cell_deps);
            tx_skeleton.cell_deps_mut().extend(cell_deps);
        }
        tx_skeleton.inputs_mut().push(verifier_context.input);
        if let Some(verifier_witness) = cancel_output.verifier_witness {
            tx_skeleton.witnesses_mut().push(verifier_witness);
        }

        // Recover Accounts
        if let Some(recover_accounts_context) = verifier_context.recover_accounts_context {
            let RecoverAccountsContext {
                cell_deps,
                inputs,
                witnesses,
            } = recover_accounts_context;

            // append dummy witness to align recover account witness (verifier may not have witness)
            let input_len = tx_skeleton.inputs().len();
            let witness_len = tx_skeleton.witnesses_mut().len();
            if input_len != witness_len {
                // append dummy witness args to align our reverted deposit witness args
                let dummy_witness_argses = (0..input_len - witness_len)
                    .into_iter()
                    .map(|_| WitnessArgs::default())
                    .collect::<Vec<_>>();
                tx_skeleton.witnesses_mut().extend(dummy_witness_argses);
            }

            tx_skeleton.cell_deps_mut().extend(cell_deps);
            tx_skeleton.inputs_mut().extend(inputs);
            tx_skeleton.witnesses_mut().extend(witnesses);
        }

        // Burn
        tx_skeleton.outputs_mut().extend(cancel_output.burn_cells);

        // Signature verification needs an owner cell
        if !has_lock_cell(&tx_skeleton, self.wallet.lock_script()) {
            let spent_inputs = verifier_context.spent_inputs;

            let owner_input = self
                .query_owner_cell_for_verifier(verifier_tx_hash, spent_inputs)
                .await?;
            log::debug!("push an owner cell to unlock verifier cell");

            let owner_lock_dep = self.ckb_genesis_info.sighash_dep();
            tx_skeleton.cell_deps_mut().push(owner_lock_dep);
            tx_skeleton.inputs_mut().push(owner_input);
        }

        // ensure no cell dep duplicate
        {
            let deps: HashSet<_> = tx_skeleton.cell_deps_mut().iter().collect();
            *tx_skeleton.cell_deps_mut() = deps.into_iter().cloned().collect();
        }

        let owner_lock = self.wallet.lock_script().to_owned();
        fill_tx_fee(
            &mut tx_skeleton,
            &self.rpc_client.indexer,
            owner_lock,
            self.config.fee_rate,
        )
        .await?;
        self.wallet.sign_tx_skeleton(tx_skeleton)
    }

    async fn query_owner_cell_for_verifier(
        &self,
        verifier_tx_hash: H256,
        spent_inputs: Option<HashSet<OutPoint>>,
    ) -> Result<InputCellInfo> {
        let rpc_client = &self.rpc_client;
        let owner_lock = self.wallet.lock_script().to_owned();

        if let Ok(Some(cell)) = rpc_client.query_owner_cell(owner_lock, spent_inputs).await {
            return Ok(to_input_cell_info(cell));
        }

        log::debug!("can't find a owner cell for verifier, try wait verifier tx committed");
        tokio::time::timeout(
            Duration::from_secs(30),
            self.rpc_client.ckb.wait_tx_committed(verifier_tx_hash),
        )
        .await
        .with_context(|| format!("wait for tx committed 0x{}", to_hex(&verifier_tx_hash)))??;

        let owner_lock = self.wallet.lock_script().to_owned();
        let cell = {
            let query = rpc_client.query_owner_cell(owner_lock, None).await?;
            query.ok_or_else(|| anyhow!("can't find an owner cell for verifier"))?
        };

        Ok(to_input_cell_info(cell))
    }

    async fn dry_run_transaction(&self, tx: &Transaction, action: &str) -> Result<()> {
        match self.rpc_client.dry_run_transaction(tx).await {
            Ok(cycles) => {
                log::info!("tx({}) {} cycles: {}", action, tx.hash().pack(), cycles);
                Ok(())
            }
            Err(err) => {
                log::error!("dry run {} tx {} failed", action, tx.hash().pack());

                let err_str = err.to_string();
                if err_str.contains(TRANSACTION_FAILED_TO_RESOLVE_ERROR) {
                    if let Err(err) = self.contracts_dep_manager.refresh().await {
                        // Lets retry on next error
                        log::error!("[contracts dep] refresh failed {}", err);
                    }
                    return Ok(());
                }
                Err(err)
            }
        }
    }
}

struct RollupState {
    rollup_cell: CellInfo,
    inner: GlobalState,
}

impl RollupState {
    async fn query(rpc_client: &RPCClient) -> Result<Self> {
        let query_cell = rpc_client.query_rollup_cell().await?;

        let rollup_cell = query_cell.ok_or_else(|| anyhow!("rollup cell not found"))?;
        let global_state = global_state_from_slice(&rollup_cell.data)?;

        Ok(RollupState {
            rollup_cell,
            inner: global_state,
        })
    }

    fn rollup_input(&self) -> InputCellInfo {
        to_input_cell_info(self.rollup_cell.clone())
    }

    fn rollup_output(&self) -> CellOutput {
        self.rollup_cell.output.clone()
    }

    fn get_state(&self) -> &GlobalState {
        &self.inner
    }

    fn status(&self) -> Result<Status> {
        let status: u8 = self.inner.status().into();
        Status::try_from(status).map_err(|n| anyhow!("invalid status {}", n))
    }
}

#[derive(Clone)]
struct VerifierContext {
    cell_dep: CellDep,
    input: InputCellInfo,
    witness: Option<WitnessArgs>,
    load_data_context: Option<LoadDataContext>,
    recover_accounts_context: Option<RecoverAccountsContext>,
    spent_inputs: Option<HashSet<OutPoint>>,
}

impl VerifierContext {
    fn new(
        cell_dep: CellDep,
        input: InputCellInfo,
        witness: Option<WitnessArgs>,
        load_data_context: Option<LoadDataContext>,
        recover_accounts_context: Option<RecoverAccountsContext>,
        spent_inputs: Option<HashSet<OutPoint>>,
    ) -> Self {
        VerifierContext {
            cell_dep,
            input,
            witness,
            load_data_context,
            recover_accounts_context,
            spent_inputs,
        }
    }

    fn tx_hash(&self) -> H256 {
        self.input.input.previous_output().tx_hash().unpack()
    }
}

fn has_lock_cell(tx_skeleton: &TransactionSkeleton, lock: &Script) -> bool {
    let lock_hash = lock.hash();
    let mut inputs = tx_skeleton.inputs().iter();
    inputs.any(|input| input.cell.output.lock().hash() == lock_hash)
}

fn to_tip_number(event: &ChainEvent) -> u64 {
    let tip_block = match event {
        ChainEvent::Reverted {
            old_tip: _,
            new_block,
        } => new_block,
        ChainEvent::NewBlock { block } => block,
    };
    tip_block.header().raw().number().unpack()
}

fn to_input_cell_info(cell_info: CellInfo) -> InputCellInfo {
    InputCellInfo {
        input: CellInput::new_builder()
            .previous_output(cell_info.out_point.clone())
            .build(),
        cell: cell_info,
    }
}

fn to_input_cell_info_with_since(cell_info: CellInfo, since: u64) -> InputCellInfo {
    InputCellInfo {
        input: CellInput::new_builder()
            .previous_output(cell_info.out_point.clone())
            .since(since.pack())
            .build(),
        cell: cell_info,
    }
}

fn to_hex(hash: &H256) -> String {
    hex::encode(hash.as_slice())
}

fn to_cell_info(cell: ChallengeCell) -> CellInfo {
    CellInfo {
        out_point: cell.input.previous_output(),
        output: cell.output,
        data: cell.output_data,
    }
}

fn extract_inputs(tx: &Transaction) -> HashSet<OutPoint> {
    let inputs = tx.raw().inputs().into_iter();
    inputs.map(|i| i.previous_output()).collect()
}

fn extract_challenge_target(cell: &ChallengeCell) -> Result<ChallengeTarget> {
    let lock_args = {
        let args: Bytes = cell.output.lock().args().unpack();
        match ChallengeLockArgsReader::verify(&args.slice(32..), false) {
            Ok(_) => ChallengeLockArgs::new_unchecked(args.slice(32..)),
            Err(err) => return Err(anyhow!("invalid challenge lock args {}", err)),
        }
    };

    Ok(lock_args.target())
}

fn validate_load_data_strategy_offchain(
    mock_context: &OffChainMockContext,
    global_state: GlobalState,
    challenge_cell: &ChallengeCell,
    context: VerifyContext,
) -> Result<LoadDataStrategy> {
    let challenge_target = extract_challenge_target(challenge_cell)?;

    let verify = |strategy: LoadDataStrategy| -> Result<_> {
        log::debug!("validate cancel challenge with strategy {:?}", strategy);

        let mock_output = mock_cancel_challenge_tx(
            &mock_context.mock_rollup,
            global_state.clone(),
            challenge_target.clone(),
            context.clone(),
            Some(strategy),
        )?;

        if mock_output.tx.as_slice().len() as u64 > MAX_CANCEL_TX_BYTES {
            bail!("cancel tx max bytes exceeded");
        }

        verify_tx(
            &mock_context.rollup_cell_deps,
            TxWithContext::from(mock_output),
            MAX_CANCEL_CYCLES,
        )?;

        Ok(strategy)
    };

    match verify(LoadDataStrategy::Witness) {
        Err(err) => log::warn!("cancel challenge by witness {}, try cell dep", err),
        Ok(_) => return Ok(LoadDataStrategy::Witness),
    }

    verify(LoadDataStrategy::CellDep)
}
