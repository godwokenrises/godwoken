use std::{
    collections::{HashMap, HashSet},
    sync::{atomic::Ordering::SeqCst, Arc},
    time::Instant,
};

use crate::{
    account_lock_manage::AccountLockManage,
    backend_manage::BackendManage,
    constants::{L2TX_MAX_CYCLES, MAX_READ_DATA_BYTES_LIMIT, MAX_WRITE_DATA_BYTES_LIMIT},
    error::{BlockError, TransactionValidateError, WithdrawalError},
    vm_cost_model::instruction_cycles,
    VMVersion,
};
use crate::{
    backend_manage::Backend,
    error::{Error, TransactionError},
    sudt::build_l2_sudt_script,
};
use crate::{error::AccountError, syscalls::L2Syscalls};
use crate::{error::LockAlgorithmError, traits::StateExt};
use arc_swap::ArcSwap;
use gw_ckb_hardfork::GLOBAL_VM_VERSION;
use gw_common::{
    builtins::CKB_SUDT_ACCOUNT_ID,
    error::Error as StateError,
    h256_ext::H256Ext,
    merkle_utils::calculate_state_checkpoint,
    state::{build_account_field_key, to_short_address, State, GW_ACCOUNT_NONCE_TYPE},
    H256,
};
use gw_dynamic_config::manager::DynamicConfigManager;
use gw_store::{state::state_db::StateContext, transaction::StoreTransaction};
use gw_traits::{ChainView, CodeStore};
use gw_types::{
    bytes::Bytes,
    core::{ChallengeTargetType, ScriptHashType},
    offchain::{RollupContext, RunResult},
    packed::{
        AccountMerkleState, BlockInfo, CellOutput, ChallengeTarget, DepositRequest, L2Block,
        L2Transaction, RawL2Block, RawL2Transaction, Script, TxReceipt, WithdrawalLockArgs,
        WithdrawalReceipt, WithdrawalRequest, WithdrawalRequestExtra,
    },
    prelude::*,
};

use ckb_vm::{DefaultMachineBuilder, SupportMachine};

#[cfg(not(has_asm))]
use ckb_vm::TraceMachine;
use tracing::instrument;

pub struct ApplyBlockArgs {
    pub l2block: L2Block,
    pub deposit_requests: Vec<DepositRequest>,
}

pub enum ApplyBlockResult {
    Success {
        withdrawal_receipts: Vec<WithdrawalReceipt>,
        prev_txs_state: AccountMerkleState,
        tx_receipts: Vec<TxReceipt>,
        offchain_used_cycles: u64,
    },
    Challenge {
        target: ChallengeTarget,
        error: Error,
    },
    Error(Error),
}

#[derive(Debug)]
pub enum WithdrawalCellError {
    MinCapacity { min: u128, req: u64 },
    OwnerLock(H256),
    V1DepositLock(H256),
}

impl From<WithdrawalCellError> for Error {
    fn from(err: WithdrawalCellError) -> Self {
        match err {
            WithdrawalCellError::MinCapacity { min, req } => AccountError::InsufficientCapacity {
                expected: min,
                actual: req,
            }
            .into(),
            WithdrawalCellError::OwnerLock(hash) => WithdrawalError::OwnerLock(hash.pack()).into(),
            WithdrawalCellError::V1DepositLock(hash) => {
                WithdrawalError::V1DepositLock(hash.pack()).into()
            }
        }
    }
}

pub enum UnlockWithdrawal {
    WithoutOwnerLock,
    WithOwnerLock { lock: Script },
    ToV1 { deposit_lock: Script },
}

impl From<&WithdrawalRequestExtra> for UnlockWithdrawal {
    fn from(extra: &WithdrawalRequestExtra) -> Self {
        match extra.opt_owner_lock() {
            None => UnlockWithdrawal::WithoutOwnerLock,
            Some(deposit_lock) if extra.withdraw_to_v1() == 1u8.into() => {
                UnlockWithdrawal::ToV1 { deposit_lock }
            }
            Some(lock) => UnlockWithdrawal::WithOwnerLock { lock },
        }
    }
}

impl From<Option<Script>> for UnlockWithdrawal {
    fn from(opt_lock: Option<Script>) -> UnlockWithdrawal {
        match opt_lock {
            Some(lock) => UnlockWithdrawal::WithOwnerLock { lock },
            None => UnlockWithdrawal::WithoutOwnerLock,
        }
    }
}

impl From<Script> for UnlockWithdrawal {
    fn from(lock: Script) -> UnlockWithdrawal {
        UnlockWithdrawal::WithOwnerLock { lock }
    }
}

pub struct Generator {
    backend_manage: BackendManage,
    account_lock_manage: AccountLockManage,
    rollup_context: RollupContext,
}

impl Generator {
    pub fn new(
        backend_manage: BackendManage,
        account_lock_manage: AccountLockManage,
        rollup_context: RollupContext,
    ) -> Self {
        Generator {
            backend_manage,
            account_lock_manage,
            rollup_context,
        }
    }

    pub fn rollup_context(&self) -> &RollupContext {
        &self.rollup_context
    }

    pub fn account_lock_manage(&self) -> &AccountLockManage {
        &self.account_lock_manage
    }

    #[instrument(skip_all, fields(backend = ?backend.backend_type))]
    fn machine_run<'a, S: State + CodeStore, C: ChainView>(
        &'a self,
        chain: &'a C,
        state: &'a S,
        block_info: &'a BlockInfo,
        raw_tx: &'a RawL2Transaction,
        max_cycles: u64,
        backend: Backend,
    ) -> Result<RunResult, TransactionError> {
        let mut run_result = RunResult::default();
        let used_cycles;
        let exit_code;

        {
            let t = Instant::now();
            let global_vm_version = GLOBAL_VM_VERSION.load(SeqCst);
            let vm_version = match global_vm_version {
                0 => VMVersion::V0,
                1 => VMVersion::V1,
                ver => panic!("Unsupport VMVersion: {}", ver),
            };
            let core_machine = vm_version.init_core_machine(max_cycles);
            let machine_builder = DefaultMachineBuilder::new(core_machine)
                .syscall(Box::new(L2Syscalls {
                    chain,
                    state,
                    block_info,
                    raw_tx,
                    rollup_context: &self.rollup_context,
                    account_lock_manage: &self.account_lock_manage,
                    result: &mut run_result,
                    code_store: state,
                }))
                .instruction_cycle_func(Box::new(instruction_cycles));
            let default_machine = machine_builder.build();

            #[cfg(has_asm)]
            let aot_code_opt = self
                .backend_manage
                .get_aot_code(&backend.validator_script_type_hash, global_vm_version);
            #[cfg(feature = "aot")]
            if aot_code_opt.is_none() {
                log::warn!("[machine_run] Not AOT mode!");
            }

            #[cfg(has_asm)]
            let mut machine = ckb_vm::machine::asm::AsmMachine::new(default_machine, aot_code_opt);

            #[cfg(not(has_asm))]
            let mut machine = TraceMachine::new(default_machine);

            machine.load_program(&backend.generator, &[])?;
            exit_code = machine.run()?;
            used_cycles = machine.machine.cycles();
            log::debug!(
                "[execute tx] VM machine_run time: {}ms, exit code: {} used_cycles: {}",
                t.elapsed().as_millis(),
                exit_code,
                used_cycles
            );
        }
        run_result.used_cycles = used_cycles;
        run_result.exit_code = exit_code;

        Ok(run_result)
    }

    /// Verify withdrawal request
    /// Notice this function do not perform signature check
    #[instrument(skip_all)]
    pub fn verify_withdrawal_request<S: State + CodeStore>(
        &self,
        state: &S,
        withdrawal_request: &WithdrawalRequest,
        asset_script: Option<Script>,
        unlock_withdrawal: UnlockWithdrawal,
    ) -> Result<(), Error> {
        let raw = withdrawal_request.raw();
        let account_script_hash: H256 = raw.account_script_hash().unpack();
        let sudt_script_hash: H256 = raw.sudt_script_hash().unpack();
        let amount: u128 = raw.amount().unpack();
        let capacity: u64 = raw.capacity().unpack();
        let fee = raw.fee();
        let fee_sudt_id: u32 = fee.sudt_id().unpack();
        let fee_amount: u128 = fee.amount().unpack();
        let account_short_address = to_short_address(&account_script_hash);

        // check capacity (use dummy block hash and number)
        let rollup_context = self.rollup_context();
        Self::build_withdrawal_cell_output(
            rollup_context,
            withdrawal_request,
            &H256::one(),
            1,
            asset_script,
            unlock_withdrawal,
        )?;

        // find user account
        let id = state
            .get_account_id_by_script_hash(&account_script_hash)?
            .ok_or(AccountError::UnknownAccount)?; // find Simple UDT account

        // check nonce
        let expected_nonce = state.get_nonce(id)?;
        let actual_nonce: u32 = raw.nonce().unpack();
        if actual_nonce != expected_nonce {
            return Err(WithdrawalError::Nonce {
                expected: expected_nonce,
                actual: actual_nonce,
            }
            .into());
        }

        // check CKB balance
        let ckb_balance = state.get_sudt_balance(CKB_SUDT_ACCOUNT_ID, account_short_address)?;
        let required_ckb_capacity = {
            let mut required_capacity = capacity as u128;
            // Count withdrawal fee
            if fee_sudt_id == CKB_SUDT_ACCOUNT_ID {
                required_capacity = required_capacity.saturating_add(fee_amount);
            }
            required_capacity
        };
        if required_ckb_capacity > ckb_balance {
            return Err(WithdrawalError::Overdraft.into());
        }
        let l2_sudt_script_hash =
            build_l2_sudt_script(&self.rollup_context, &sudt_script_hash).hash();
        let sudt_id = state
            .get_account_id_by_script_hash(&l2_sudt_script_hash.into())?
            .ok_or(AccountError::UnknownSUDT)?;
        // The sUDT id must not be equals to the CKB sUDT id if amount isn't 0
        if sudt_id != CKB_SUDT_ACCOUNT_ID {
            // check SUDT balance
            // user can't withdrawal 0 SUDT when non-CKB sudt_id exists
            if amount == 0 {
                return Err(WithdrawalError::NonPositiveSUDTAmount.into());
            }
            let mut required_amount = amount;
            if sudt_id == fee_sudt_id {
                required_amount = required_amount.saturating_add(fee_amount);
            }
            let balance = state.get_sudt_balance(sudt_id, account_short_address)?;
            if required_amount > balance {
                return Err(WithdrawalError::Overdraft.into());
            }
        } else if amount != 0 {
            // user can't withdrawal CKB token via SUDT fields
            return Err(WithdrawalError::WithdrawFakedCKB.into());
        }

        // check fees if it isn't been checked yet
        if fee_sudt_id != CKB_SUDT_ACCOUNT_ID && fee_sudt_id != sudt_id && fee_amount > 0 {
            let balance = state.get_sudt_balance(fee_sudt_id, account_short_address)?;
            if fee_amount > balance {
                return Err(WithdrawalError::Overdraft.into());
            }
        }

        Ok(())
    }

    /// Check withdrawal request signature
    #[instrument(skip_all)]
    pub fn check_withdrawal_request_signature<S: State + CodeStore>(
        &self,
        state: &S,
        withdrawal_request: &WithdrawalRequest,
    ) -> Result<(), Error> {
        let raw = withdrawal_request.raw();
        let account_script_hash: [u8; 32] = raw.account_script_hash().unpack();

        // check signature
        let account_script = state
            .get_script(&account_script_hash.into())
            .ok_or(StateError::MissingKey)?;
        let lock_code_hash: [u8; 32] = account_script.code_hash().unpack();
        let lock_algo = self
            .account_lock_manage
            .get_lock_algorithm(&lock_code_hash.into())
            .ok_or(LockAlgorithmError::UnknownAccountLock)?;

        let message = raw.calc_message(&self.rollup_context.rollup_script_hash);
        let valid_signature = lock_algo.verify_message(
            account_script.args().unpack(),
            withdrawal_request.signature().unpack(),
            message,
        )?;

        if !valid_signature {
            return Err(LockAlgorithmError::InvalidSignature.into());
        }

        Ok(())
    }

    /// verify transaction
    /// Notice this function do not perform signature check
    #[instrument(skip_all)]
    pub fn verify_transaction<S: State + CodeStore>(
        &self,
        state: &S,
        tx: &L2Transaction,
    ) -> Result<(), TransactionValidateError> {
        let raw_tx = tx.raw();
        let sender_id: u32 = raw_tx.from_id().unpack();

        // verify nonce
        let account_nonce: u32 = state.get_nonce(sender_id)?;
        let nonce: u32 = raw_tx.nonce().unpack();
        if nonce != account_nonce {
            return Err(TransactionError::Nonce {
                expected: account_nonce,
                actual: nonce,
                account_id: sender_id,
            }
            .into());
        }

        Ok(())
    }

    // Check transaction signature
    #[instrument(skip_all)]
    pub fn check_transaction_signature<S: State + CodeStore>(
        &self,
        state: &S,
        tx: &L2Transaction,
    ) -> Result<(), TransactionValidateError> {
        let raw_tx = tx.raw();
        let sender_id: u32 = raw_tx.from_id().unpack();
        let receiver_id: u32 = raw_tx.to_id().unpack();

        // verify signature
        let script_hash = state.get_script_hash(sender_id)?;
        if script_hash.is_zero() {
            return Err(AccountError::ScriptNotFound {
                account_id: sender_id,
            }
            .into());
        }
        let script = state.get_script(&script_hash).expect("get script");
        let lock_code_hash: [u8; 32] = script.code_hash().unpack();

        let receiver_script_hash = state.get_script_hash(receiver_id)?;
        if receiver_script_hash.is_zero() {
            return Err(AccountError::ScriptNotFound {
                account_id: receiver_id,
            }
            .into());
        }
        let receiver_script = state
            .get_script(&receiver_script_hash)
            .expect("get receiver script");

        let lock_algo = self
            .account_lock_manage()
            .get_lock_algorithm(&lock_code_hash.into())
            .ok_or(LockAlgorithmError::UnknownAccountLock)?;
        let valid_signature =
            lock_algo.verify_tx(&self.rollup_context, script, receiver_script, tx)?;
        if !valid_signature {
            return Err(LockAlgorithmError::InvalidSignature.into());
        }
        Ok(())
    }

    /// Apply l2 state transition
    #[instrument(skip_all, fields(block = args.l2block.raw().number().unpack(), deposits_count = args.deposit_requests.len()))]
    pub fn verify_and_apply_block<C: ChainView>(
        &self,
        db: &StoreTransaction,
        chain: &C,
        args: ApplyBlockArgs,
        skipped_invalid_block_list: &HashSet<H256>,
    ) -> ApplyBlockResult {
        let raw_block = args.l2block.raw();
        let block_info = get_block_info(&raw_block);
        let block_number = raw_block.number().unpack();

        let mut state = match db.state_tree(StateContext::AttachBlock(block_number)) {
            Ok(state) => state,
            Err(err) => {
                log::error!("next state {}", err);
                return ApplyBlockResult::Error(Error::State(StateError::Store));
            }
        };

        // apply withdrawal to state
        let withdrawal_requests: Vec<_> = args.l2block.withdrawals().into_iter().collect();
        let block_hash = raw_block.hash();
        let block_producer_id: u32 = block_info.block_producer_id().unpack();
        let state_checkpoint_list: Vec<H256> = raw_block.state_checkpoint_list().unpack();

        let mut check_signature_total_ms = 0;
        let mut execute_tx_total_ms = 0;
        let mut apply_state_total_ms = 0;
        let mut withdrawal_receipts = Vec::with_capacity(withdrawal_requests.len());
        for (wth_idx, request) in withdrawal_requests.into_iter().enumerate() {
            let now = Instant::now();
            if let Err(error) = self.check_withdrawal_request_signature(&state, &request) {
                let target = build_challenge_target(
                    block_hash.into(),
                    ChallengeTargetType::Withdrawal,
                    wth_idx as u32,
                );

                return ApplyBlockResult::Challenge { target, error };
            }
            check_signature_total_ms += now.elapsed().as_millis();

            let withdrawal_receipt = match state.apply_withdrawal_request(
                &self.rollup_context,
                block_producer_id,
                &request,
            ) {
                Ok(receipt) => receipt,
                Err(err) => return ApplyBlockResult::Error(err),
            };
            let account_state = state.get_merkle_state();
            let expected_checkpoint = calculate_state_checkpoint(
                &account_state.merkle_root().unpack(),
                account_state.count().unpack(),
            );
            let block_checkpoint: H256 = match state_checkpoint_list.get(wth_idx) {
                Some(checkpoint) => *checkpoint,
                None => {
                    return ApplyBlockResult::Error(
                        BlockError::CheckpointNotFound { index: wth_idx }.into(),
                    );
                }
            };
            // since the state-validator script will verify withdrawals, we should always pass this check
            assert_eq!(
                block_checkpoint, expected_checkpoint,
                "check withdrawal checkpoint"
            );
            withdrawal_receipts.push(withdrawal_receipt)
        }

        // apply deposition to state
        if let Err(err) = state.apply_deposit_requests(&self.rollup_context, &args.deposit_requests)
        {
            return ApplyBlockResult::Error(err);
        }

        let prev_txs_state = state.get_merkle_state();

        // handle transactions
        let mut offchain_used_cycles: u64 = 0;
        let mut tx_receipts = Vec::with_capacity(args.l2block.transactions().len());
        let skip_checkpoint_check = skipped_invalid_block_list.contains(&block_hash.into());
        if skip_checkpoint_check {
            log::warn!(
                "skip the checkpoint check of block: #{} {}",
                block_number,
                hex::encode(&block_hash)
            );
        }
        for (tx_index, tx) in args.l2block.transactions().into_iter().enumerate() {
            log::debug!(
                "[apply block] execute tx index: {} hash: {}",
                tx_index,
                hex::encode(tx.hash())
            );
            let now = Instant::now();
            if let Err(err) = self.check_transaction_signature(&state, &tx) {
                let target = build_challenge_target(
                    block_hash.into(),
                    ChallengeTargetType::TxSignature,
                    tx_index as u32,
                );

                return ApplyBlockResult::Challenge {
                    target,
                    error: err.into(),
                };
            }
            check_signature_total_ms += now.elapsed().as_millis();

            // check nonce
            let raw_tx = tx.raw();
            let expected_nonce = match state.get_nonce(raw_tx.from_id().unpack()) {
                Err(err) => return ApplyBlockResult::Error(Error::from(err)),
                Ok(nonce) => nonce,
            };
            let actual_nonce: u32 = raw_tx.nonce().unpack();
            if actual_nonce != expected_nonce {
                return ApplyBlockResult::Challenge {
                    target: build_challenge_target(
                        block_hash.into(),
                        ChallengeTargetType::TxExecution,
                        tx_index as u32,
                    ),
                    error: Error::Transaction(TransactionError::Nonce {
                        expected: expected_nonce,
                        actual: actual_nonce,
                        account_id: raw_tx.from_id().unpack(),
                    }),
                };
            }

            // build call context
            // NOTICE users only allowed to send HandleMessage CallType txs
            let now = Instant::now();

            // skip whitelist validate since we are validating a committed block
            let run_result = match self.execute_transaction(
                chain,
                &state,
                &block_info,
                &raw_tx,
                L2TX_MAX_CYCLES,
                None,
            ) {
                Ok(run_result) => run_result,
                Err(err) => {
                    let target = build_challenge_target(
                        block_hash.into(),
                        ChallengeTargetType::TxExecution,
                        tx_index as u32,
                    );

                    return ApplyBlockResult::Challenge {
                        target,
                        error: Error::Transaction(err),
                    };
                }
            };
            execute_tx_total_ms += now.elapsed().as_millis();

            {
                let now = Instant::now();
                if let Err(err) = state.apply_run_result(&run_result) {
                    return ApplyBlockResult::Error(err);
                }
                apply_state_total_ms += now.elapsed().as_millis();
                let account_state = state.get_merkle_state();

                let expected_checkpoint = calculate_state_checkpoint(
                    &account_state.merkle_root().unpack(),
                    account_state.count().unpack(),
                );
                let checkpoint_index = withdrawal_receipts.len() + tx_index;
                let block_checkpoint: H256 = match state_checkpoint_list.get(checkpoint_index) {
                    Some(checkpoint) => *checkpoint,
                    None => {
                        return ApplyBlockResult::Error(
                            BlockError::CheckpointNotFound {
                                index: checkpoint_index,
                            }
                            .into(),
                        );
                    }
                };

                if !skip_checkpoint_check && block_checkpoint != expected_checkpoint {
                    let target = build_challenge_target(
                        block_hash.into(),
                        ChallengeTargetType::TxExecution,
                        tx_index as u32,
                    );
                    return ApplyBlockResult::Challenge {
                        target,
                        error: Error::Block(BlockError::InvalidCheckpoint {
                            expected_checkpoint,
                            block_checkpoint,
                            index: checkpoint_index,
                        }),
                    };
                }

                let used_cycles = run_result.used_cycles;
                let post_state = match state.merkle_state() {
                    Ok(merkle_state) => merkle_state,
                    Err(err) => return ApplyBlockResult::Error(err),
                };
                let tx_receipt =
                    TxReceipt::build_receipt(tx.witness_hash().into(), run_result, post_state);

                tx_receipts.push(tx_receipt);
                offchain_used_cycles = offchain_used_cycles.saturating_add(used_cycles);
            }
        }

        // check post state
        if !skip_checkpoint_check {
            let post_merkle_root: H256 = raw_block.post_account().merkle_root().unpack();
            let post_merkle_count: u32 = raw_block.post_account().count().unpack();
            assert_eq!(
                state.calculate_root().expect("check post root"),
                post_merkle_root,
                "post account merkle root must be consistent"
            );
            assert_eq!(
                state.get_account_count().expect("check post count"),
                post_merkle_count,
                "post account merkle count must be consistent"
            );
        }

        log::debug!(
            "signature {}ms execute tx {}ms apply state {}ms",
            check_signature_total_ms,
            execute_tx_total_ms,
            apply_state_total_ms
        );

        ApplyBlockResult::Success {
            withdrawal_receipts,
            prev_txs_state,
            tx_receipts,
            offchain_used_cycles,
        }
    }

    #[instrument(skip_all, fields(script_hash = %script_hash.pack()))]
    pub fn load_backend<S: State + CodeStore>(
        &self,
        state: &S,
        script_hash: &H256,
    ) -> Option<Backend> {
        log::debug!(
            "load_backend for script_hash: {}",
            hex::encode(script_hash.as_slice())
        );
        state
            .get_script(script_hash)
            .and_then(|script| {
                // only accept type script hash type for now
                if script.hash_type() == ScriptHashType::Type.into() {
                    let code_hash: [u8; 32] = script.code_hash().unpack();
                    log::debug!("load_backend by code_hash: {}", hex::encode(code_hash));
                    self.backend_manage.get_backend(&code_hash.into())
                } else {
                    log::error!(
                        "Found a invalid account script which hash_type is data: {:?}",
                        script
                    );
                    None
                }
            })
            .cloned()
    }

    /// execute a layer2 tx
    #[instrument(skip_all)]
    pub fn execute_transaction<S: State + CodeStore, C: ChainView>(
        &self,
        chain: &C,
        state: &S,
        block_info: &BlockInfo,
        raw_tx: &RawL2Transaction,
        max_cycles: u64,
        dynamic_config_manager: Option<Arc<ArcSwap<DynamicConfigManager>>>,
    ) -> Result<RunResult, TransactionError> {
        let run_result = self.unchecked_execute_transaction(
            chain,
            state,
            block_info,
            raw_tx,
            max_cycles,
            dynamic_config_manager,
        )?;
        if 0 != run_result.exit_code {
            return Err(TransactionError::InvalidExitCode(run_result.exit_code));
        }

        Ok(run_result)
    }

    /// execute a layer2 tx, doesn't check exit code
    #[instrument(skip_all, fields(block = block_info.number().unpack(), tx_hash = %raw_tx.hash().pack()))]
    pub fn unchecked_execute_transaction<S: State + CodeStore, C: ChainView>(
        &self,
        chain: &C,
        state: &S,
        block_info: &BlockInfo,
        raw_tx: &RawL2Transaction,
        max_cycles: u64,
        dynamic_config_manager: Option<Arc<ArcSwap<DynamicConfigManager>>>,
    ) -> Result<RunResult, TransactionError> {
        if let Some(polyjuice_contract_creator_allowlist) =
            dynamic_config_manager.as_ref().and_then(|manager| {
                manager
                    .load()
                    .get_polyjuice_contract_creator_allowlist()
                    .to_owned()
            })
        {
            use gw_tx_filter::polyjuice_contract_creator_allowlist::Error;
            match polyjuice_contract_creator_allowlist.validate_with_state(state, raw_tx) {
                Ok(_) => (),
                Err(Error::Common(err)) => return Err(TransactionError::from(err)),
                Err(Error::ScriptHashNotFound) => return Err(TransactionError::ScriptHashNotFound),
                Err(Error::PermissionDenied { account_id }) => {
                    return Err(TransactionError::InvalidContractCreatorAccount {
                        backend: "polyjuice",
                        account_id,
                    })
                }
            }
        }

        let sender_id: u32 = raw_tx.from_id().unpack();
        let nonce_before_execution = state.get_nonce(sender_id)?;

        let account_id = raw_tx.to_id().unpack();
        let script_hash = state.get_script_hash(account_id)?;
        let backend = self
            .load_backend(state, &script_hash)
            .ok_or(TransactionError::BackendNotFound { script_hash })?;

        let run_result: RunResult =
            self.machine_run(chain, state, block_info, raw_tx, max_cycles, backend)?;

        if 0 == run_result.exit_code {
            // check nonce is increased by backends
            let nonce_after_execution = {
                let nonce_raw_key = build_account_field_key(sender_id, GW_ACCOUNT_NONCE_TYPE);
                let value = run_result
                    .write_values
                    .get(&nonce_raw_key)
                    .ok_or(TransactionError::BackendMustIncreaseNonce)?;
                value.to_u32()
            };
            if nonce_after_execution <= nonce_before_execution {
                log::error!(
                    "nonce should increased by backends nonce before: {}, nonce after: {}",
                    nonce_before_execution,
                    nonce_after_execution
                );
                return Err(TransactionError::BackendMustIncreaseNonce);
            }
        }

        // check write data bytes
        let write_data_bytes: usize = run_result.write_data.values().map(|data| data.len()).sum();
        if write_data_bytes > MAX_WRITE_DATA_BYTES_LIMIT {
            return Err(TransactionError::ExceededMaxWriteData {
                max_bytes: MAX_WRITE_DATA_BYTES_LIMIT,
                used_bytes: write_data_bytes,
            });
        }
        // check read data bytes
        let read_data_bytes: usize = run_result.read_data.values().map(Vec::len).sum();
        if read_data_bytes > MAX_READ_DATA_BYTES_LIMIT {
            return Err(TransactionError::ExceededMaxReadData {
                max_bytes: MAX_READ_DATA_BYTES_LIMIT,
                used_bytes: read_data_bytes,
            });
        }
        // check account id of sudt proxy contract creator is from whitelist
        let from_id = raw_tx.from_id().unpack();
        if let Some(manager) = dynamic_config_manager.as_ref() {
            if !manager
                .load()
                .get_sudt_proxy_account_whitelist()
                .validate(&run_result, from_id)
            {
                return Err(TransactionError::InvalidSUDTProxyCreatorAccount {
                    account_id: from_id,
                });
            }
        }
        Ok(run_result)
    }

    pub fn build_withdrawal_cell_output(
        rollup_context: &RollupContext,
        req: &WithdrawalRequest,
        block_hash: &H256,
        block_number: u64,
        opt_asset_script: Option<Script>,
        unlock_withdrawal: UnlockWithdrawal,
    ) -> Result<(CellOutput, Bytes), WithdrawalCellError> {
        let withdrawal_capacity: u64 = req.raw().capacity().unpack();
        let lock_args: Bytes = {
            let withdrawal_lock_args = WithdrawalLockArgs::new_builder()
                .account_script_hash(req.raw().account_script_hash())
                .withdrawal_block_hash(Into::<[u8; 32]>::into(*block_hash).pack())
                .withdrawal_block_number(block_number.pack())
                .sudt_script_hash(req.raw().sudt_script_hash())
                .sell_amount(req.raw().sell_amount())
                .sell_capacity(req.raw().sell_capacity())
                .owner_lock_hash(req.raw().owner_lock_hash())
                .payment_lock_hash(req.raw().payment_lock_hash())
                .build();

            let mut args = Vec::new();
            args.extend_from_slice(rollup_context.rollup_script_hash.as_slice());
            args.extend_from_slice(withdrawal_lock_args.as_slice());
            if let UnlockWithdrawal::WithOwnerLock {
                lock: ref owner_lock,
            } = unlock_withdrawal
            {
                let owner_lock_hash: [u8; 32] = req.raw().owner_lock_hash().unpack();
                if owner_lock_hash != owner_lock.hash() {
                    return Err(WithdrawalCellError::OwnerLock(owner_lock_hash.into()));
                }

                args.extend_from_slice(&(owner_lock.as_slice().len() as u32).to_be_bytes());
                args.extend_from_slice(owner_lock.as_slice());
            }
            if let UnlockWithdrawal::ToV1 { ref deposit_lock } = unlock_withdrawal {
                let owner_lock_hash: [u8; 32] = req.raw().owner_lock_hash().unpack();
                if owner_lock_hash != deposit_lock.hash() {
                    return Err(WithdrawalCellError::V1DepositLock(owner_lock_hash.into()));
                }

                args.extend_from_slice(&(deposit_lock.as_slice().len() as u32).to_be_bytes());
                args.extend_from_slice(deposit_lock.as_slice());
                args.push(1u8);
            }

            Bytes::from(args)
        };

        let lock = Script::new_builder()
            .code_hash(rollup_context.rollup_config.withdrawal_script_type_hash())
            .hash_type(ScriptHashType::Type.into())
            .args(lock_args.pack())
            .build();

        let (type_, data) = match opt_asset_script {
            Some(type_) => (Some(type_).pack(), req.raw().amount().as_bytes()),
            None => (None::<Script>.pack(), Bytes::new()),
        };

        let output = CellOutput::new_builder()
            .capacity(withdrawal_capacity.pack())
            .type_(type_)
            .lock(lock)
            .build();

        match output.occupied_capacity(data.len()) {
            Ok(min_capacity) if min_capacity > withdrawal_capacity => {
                Err(WithdrawalCellError::MinCapacity {
                    min: min_capacity as u128,
                    req: req.raw().capacity().unpack(),
                })
            }
            Err(err) => {
                log::debug!("calculate withdrawal capacity {}", err); // Overflow
                Err(WithdrawalCellError::MinCapacity {
                    min: u64::MAX as u128 + 1,
                    req: req.raw().capacity().unpack(),
                })
            }
            _ => Ok((output, data)),
        }
    }

    pub fn get_backends(&self) -> &HashMap<H256, Backend> {
        self.backend_manage.get_backends()
    }
}

fn get_block_info(l2block: &RawL2Block) -> BlockInfo {
    BlockInfo::new_builder()
        .block_producer_id(l2block.block_producer_id())
        .number(l2block.number())
        .timestamp(l2block.timestamp())
        .build()
}

fn build_challenge_target(
    block_hash: H256,
    target_type: ChallengeTargetType,
    target_index: u32,
) -> ChallengeTarget {
    let target_type: u8 = target_type.into();
    ChallengeTarget::new_builder()
        .block_hash(block_hash.pack())
        .target_index(target_index.pack())
        .target_type(target_type.into())
        .build()
}

#[cfg(test)]
mod test {
    use gw_common::h256_ext::H256Ext;
    use gw_common::H256;
    use gw_types::bytes::Bytes;
    use gw_types::core::ScriptHashType;
    use gw_types::offchain::RollupContext;
    use gw_types::packed::{Fee, RawWithdrawalRequest, RollupConfig, Script, WithdrawalRequest};
    use gw_types::prelude::{Builder, Entity, Pack, Unpack};

    use crate::generator::{UnlockWithdrawal, WithdrawalCellError};
    use crate::Generator;

    #[test]
    fn test_build_withdrawal_cell_output() {
        let rollup_context = RollupContext {
            rollup_script_hash: H256::from_u32(1),
            rollup_config: RollupConfig::new_builder()
                .withdrawal_script_type_hash(H256::from_u32(100).pack())
                .build(),
        };
        let sudt_script = Script::new_builder()
            .code_hash(H256::from_u32(1).pack())
            .args(vec![3; 32].pack())
            .build();
        let owner_lock = Script::new_builder()
            .code_hash(H256::from_u32(4).pack())
            .args(vec![5; 32].pack())
            .build();

        // ## Fulfill withdrawal request
        let req = {
            let fee = Fee::new_builder()
                .sudt_id(20u32.pack())
                .amount(50u128.pack())
                .build();
            let raw = RawWithdrawalRequest::new_builder()
                .nonce(1u32.pack())
                .capacity((500 * 10u64.pow(8)).pack())
                .amount(20u128.pack())
                .sudt_script_hash(sudt_script.hash().pack())
                .account_script_hash(H256::from_u32(10).pack())
                .sell_amount(99999u128.pack())
                .sell_capacity(99999u64.pack())
                .owner_lock_hash(owner_lock.hash().pack())
                .payment_lock_hash(owner_lock.hash().pack())
                .fee(fee)
                .build();
            WithdrawalRequest::new_builder()
                .raw(raw)
                .signature(vec![6u8; 65].pack())
                .build()
        };

        let block_hash = H256::from_u32(11);
        let block_number = 11u64;
        let (output, data) = Generator::build_withdrawal_cell_output(
            &rollup_context,
            &req,
            &block_hash,
            block_number,
            Some(sudt_script.clone()),
            UnlockWithdrawal::from(owner_lock.clone()),
        )
        .unwrap();

        // Basic check
        assert_eq!(output.capacity().unpack(), req.raw().capacity().unpack());
        assert_eq!(
            output.type_().as_slice(),
            Some(sudt_script.clone()).pack().as_slice()
        );
        assert_eq!(
            output.lock().code_hash(),
            rollup_context.rollup_config.withdrawal_script_type_hash()
        );
        assert_eq!(output.lock().hash_type(), ScriptHashType::Type.into());
        assert_eq!(data, req.raw().amount().as_bytes());

        // Check lock args
        let parsed_args =
            gw_utils::withdrawal::parse_lock_args(&output.lock().args().unpack()).unwrap();
        assert_eq!(
            parsed_args.rollup_type_hash.pack(),
            rollup_context.rollup_script_hash.pack()
        );
        assert_eq!(
            parsed_args.opt_owner_lock.map(|l| l.hash()),
            Some(owner_lock.hash())
        );
        assert!(!parsed_args.withdraw_to_v1);

        let lock_args = parsed_args.lock_args.clone();
        assert_eq!(
            lock_args.account_script_hash(),
            req.raw().account_script_hash()
        );
        assert_eq!(lock_args.withdrawal_block_hash(), block_hash.pack());
        assert_eq!(lock_args.withdrawal_block_number().unpack(), block_number);
        assert_eq!(lock_args.sudt_script_hash(), sudt_script.hash().pack());
        assert_eq!(
            lock_args.sell_amount().unpack(),
            req.raw().sell_amount().unpack()
        );
        assert_eq!(
            lock_args.sell_capacity().unpack(),
            req.raw().sell_capacity().unpack()
        );
        assert_eq!(lock_args.owner_lock_hash(), owner_lock.hash().pack());
        assert_eq!(lock_args.payment_lock_hash(), owner_lock.hash().pack());

        // ## Withdraw to V1
        let (v1_output, _v1_data) = Generator::build_withdrawal_cell_output(
            &rollup_context,
            &req,
            &block_hash,
            block_number,
            Some(sudt_script.clone()),
            UnlockWithdrawal::ToV1 {
                deposit_lock: owner_lock.clone(),
            },
        )
        .unwrap();
        let parsed_to_v1_args =
            gw_utils::withdrawal::parse_lock_args(&v1_output.lock().args().unpack()).unwrap();
        assert_eq!(
            parsed_to_v1_args.rollup_type_hash.pack(),
            rollup_context.rollup_script_hash.pack()
        );
        assert_eq!(
            parsed_to_v1_args.opt_owner_lock.map(|l| l.hash()),
            Some(owner_lock.hash())
        );
        assert!(parsed_to_v1_args.withdraw_to_v1);

        // ## None asset script
        let (output2, data2) = Generator::build_withdrawal_cell_output(
            &rollup_context,
            &req,
            &block_hash,
            block_number,
            None,
            UnlockWithdrawal::from(owner_lock.clone()),
        )
        .unwrap();

        assert!(output2.type_().to_opt().is_none());
        assert_eq!(data2, Bytes::new());

        assert_eq!(output2.capacity().unpack(), output.capacity().unpack());
        assert_eq!(output2.lock().hash(), output.lock().hash());

        // ## None owner script
        let (output3, data3) = Generator::build_withdrawal_cell_output(
            &rollup_context,
            &req,
            &block_hash,
            block_number,
            Some(sudt_script.clone()),
            UnlockWithdrawal::WithoutOwnerLock,
        )
        .unwrap();

        let parsed_args3 =
            gw_utils::withdrawal::parse_lock_args(&output3.lock().args().unpack()).unwrap();
        assert!(parsed_args3.opt_owner_lock.is_none());

        assert_eq!(output3.capacity().unpack(), output.capacity().unpack());
        assert_eq!(data3, data);
        assert_eq!(output3.type_().as_slice(), output.type_().as_slice());
        assert_eq!(
            output3.lock().code_hash(),
            rollup_context.rollup_config.withdrawal_script_type_hash()
        );
        assert_eq!(output3.lock().hash_type(), ScriptHashType::Type.into());
        assert_eq!(parsed_args3.rollup_type_hash, parsed_args.rollup_type_hash);
        assert_eq!(
            parsed_args3.lock_args.as_slice(),
            parsed_args.lock_args.as_slice()
        );

        // ## Min capacity error
        let err_req = {
            let raw = req.raw().as_builder();
            let err_raw = raw
                .capacity(500u64.pack()) // ERROR: capacity not enough
                .build();
            req.clone().as_builder().raw(err_raw).build()
        };

        let err = Generator::build_withdrawal_cell_output(
            &rollup_context,
            &err_req,
            &block_hash,
            block_number,
            Some(sudt_script.clone()),
            UnlockWithdrawal::from(owner_lock),
        )
        .unwrap_err();
        assert!(matches!(
            err,
            WithdrawalCellError::MinCapacity { req: _, min: _ }
        ));
        if let WithdrawalCellError::MinCapacity { req, min: _ } = err {
            assert_eq!(req, 500);
        }

        // ## Owner lock error
        let err_owner_lock = Script::new_builder()
            .code_hash([100u8; 32].pack())
            .hash_type(ScriptHashType::Data.into())
            .args(vec![99u8; 32].pack())
            .build();
        let err = Generator::build_withdrawal_cell_output(
            &rollup_context,
            &req,
            &block_hash,
            block_number,
            Some(sudt_script),
            UnlockWithdrawal::from(err_owner_lock),
        )
        .unwrap_err();

        assert!(matches!(err, WithdrawalCellError::OwnerLock(_)));
        if let WithdrawalCellError::OwnerLock(owner_lock_hash) = err {
            assert_eq!(req.raw().owner_lock_hash(), owner_lock_hash.pack());
        }
    }
}
