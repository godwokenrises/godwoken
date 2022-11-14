use anyhow::{Context, Result};
use std::{collections::HashSet, sync::Arc, time::Instant};

use crate::{
    account_lock_manage::AccountLockManage,
    backend_manage::BackendManage,
    error::{BlockError, TransactionValidateError, WithdrawalError},
    syscalls::RunContext,
    typed_transaction::types::TypedRawTransaction,
    types::vm::VMVersion,
    utils::{get_polyjuice_creator_id, get_tx_type},
    vm_cost_model::instruction_cycles,
};
use crate::{
    backend_manage::Backend,
    error::{Error, TransactionError},
};
use crate::{error::AccountError, syscalls::L2Syscalls};
use crate::{error::LockAlgorithmError, traits::StateExt};
use arc_swap::ArcSwapOption;
use gw_common::{
    builtins::{CKB_SUDT_ACCOUNT_ID, ETH_REGISTRY_ACCOUNT_ID},
    error::Error as StateError,
    h256_ext::H256Ext,
    registry_address::RegistryAddress,
    state::{build_account_key, State, SUDT_TOTAL_SUPPLY_KEY},
    H256,
};

use gw_config::{ContractLogConfig, ForkConfig, SyscallCyclesConfig};
use gw_store::{
    state::{history::history_state::RWConfig, traits::JournalDB, BlockStateDB},
    transaction::StoreTransaction,
};
use gw_traits::{ChainView, CodeStore};
use gw_types::{
    bytes::Bytes,
    core::{ChallengeTargetType, ScriptHashType},
    offchain::{CycleMeter, RunResult},
    packed::{
        AccountMerkleState, BlockInfo, ChallengeTarget, DepositInfoVec, L2Block, L2Transaction,
        LogItem, RawL2Block, RawL2Transaction, TxReceipt, WithdrawalReceipt,
        WithdrawalRequestExtra,
    },
    prelude::*,
};
use gw_utils::RollupContext;

use ckb_vm::{DefaultMachineBuilder, SupportMachine};

#[cfg(not(has_asm))]
use ckb_vm::TraceMachine;
use gw_utils::script_log::{generate_polyjuice_system_log, GW_LOG_POLYJUICE_SYSTEM};
use tracing::instrument;

pub struct ApplyBlockArgs {
    pub l2block: L2Block,
    pub deposit_info_vec: DepositInfoVec,
    pub withdrawals: Vec<WithdrawalRequestExtra>,
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
    Error(anyhow::Error),
}

#[derive(Debug)]
pub enum WithdrawalCellError {
    MinCapacity { min: u128, req: u64 },
    OwnerLock(H256),
}

impl From<WithdrawalCellError> for Error {
    fn from(err: WithdrawalCellError) -> Self {
        match err {
            WithdrawalCellError::MinCapacity { min, req } => {
                WithdrawalError::InsufficientCapacity {
                    expected: min,
                    actual: req,
                }
                .into()
            }
            WithdrawalCellError::OwnerLock(hash) => WithdrawalError::OwnerLock(hash.pack()).into(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct CyclesPool {
    limit: u64,
    available_cycles: u64,
    syscall_config: SyscallCyclesConfig,
}

impl CyclesPool {
    pub fn new(limit: u64, syscall_config: SyscallCyclesConfig) -> Self {
        CyclesPool {
            limit,
            available_cycles: limit,
            syscall_config,
        }
    }

    pub fn limit(&self) -> u64 {
        self.limit
    }

    pub fn available_cycles(&self) -> u64 {
        self.available_cycles
    }

    pub fn cycles_used(&self) -> u64 {
        self.limit - self.available_cycles
    }

    pub fn syscall_config(&self) -> &SyscallCyclesConfig {
        &self.syscall_config
    }

    pub fn consume_cycles(&mut self, cycles: u64) -> Option<u64> {
        let opt_available_cycles = self.available_cycles.checked_sub(cycles);
        self.available_cycles = opt_available_cycles.unwrap_or(0);

        opt_available_cycles
    }
}

pub struct MachineRunArgs<'a, C, S> {
    chain: &'a C,
    state: &'a mut S,
    block_info: &'a BlockInfo,
    raw_tx: &'a RawL2Transaction,
    max_cycles: u64,
    backend: Backend,
    cycles_pool: Option<&'a mut CyclesPool>,
}

pub struct Generator {
    backend_manage: BackendManage,
    account_lock_manage: AccountLockManage,
    rollup_context: RollupContext,
    contract_log_config: ContractLogConfig,
    polyjuice_creator_id: ArcSwapOption<u32>,
}

impl Generator {
    pub fn new(
        backend_manage: BackendManage,
        account_lock_manage: AccountLockManage,
        rollup_context: RollupContext,
        contract_log_config: ContractLogConfig,
    ) -> Self {
        Generator {
            backend_manage,
            account_lock_manage,
            rollup_context,
            contract_log_config,
            polyjuice_creator_id: ArcSwapOption::from(None),
        }
    }

    pub fn clone_with_new_backends(&self, backend_manage: BackendManage) -> Self {
        Self {
            backend_manage,
            account_lock_manage: self.account_lock_manage.clone(),
            rollup_context: self.rollup_context.clone(),
            contract_log_config: self.contract_log_config.clone(),
            polyjuice_creator_id: ArcSwapOption::from(self.polyjuice_creator_id.load_full()),
        }
    }

    pub fn rollup_context(&self) -> &RollupContext {
        &self.rollup_context
    }

    pub fn fork_config(&self) -> &ForkConfig {
        &self.rollup_context.fork_config
    }

    pub fn account_lock_manage(&self) -> &AccountLockManage {
        &self.account_lock_manage
    }

    #[instrument(skip_all, fields(backend = ?args.backend.backend_type))]
    fn machine_run<S: State + CodeStore + JournalDB, C: ChainView>(
        &self,
        args: MachineRunArgs<'_, C, S>,
    ) -> Result<RunContext, TransactionError> {
        const INVALID_CYCLES_EXIT_CODE: i8 = -1;

        let MachineRunArgs {
            chain,
            state,
            block_info,
            raw_tx,
            max_cycles,
            backend,
            mut cycles_pool,
        } = args;

        let mut context = RunContext::default();
        context.debug_log_buf.reserve(1024);
        let used_cycles;
        let exit_code;
        let org_cycles_pool = cycles_pool.as_mut().map(|p| p.clone());
        {
            let t = Instant::now();
            let core_machine = VMVersion::V1.init_core_machine(max_cycles);
            let machine_builder = DefaultMachineBuilder::new(core_machine)
                .syscall(Box::new(L2Syscalls {
                    chain,
                    state,
                    block_info,
                    raw_tx,
                    rollup_context: &self.rollup_context,
                    account_lock_manage: &self.account_lock_manage,
                    cycles_pool: &mut cycles_pool,
                    context: &mut context,
                }))
                .instruction_cycle_func(Box::new(instruction_cycles));
            let default_machine = machine_builder.build();

            #[cfg(has_asm)]
            let aot_code_opt = self
                .backend_manage
                .get_aot_code(&backend.checksum.generator);
            #[cfg(has_asm)]
            if aot_code_opt.is_none() {
                log::warn!("[machine_run] Not AOT mode!");
            }

            #[cfg(has_asm)]
            let mut machine = ckb_vm::machine::asm::AsmMachine::new(default_machine, aot_code_opt);

            #[cfg(not(has_asm))]
            let mut machine = TraceMachine::new(default_machine);

            machine.load_program(&backend.generator, &[])?;
            let maybe_ok = machine.run();
            let execution_cycles = machine.machine.cycles();
            drop(machine);

            // Subtract tx execution cycles.
            if let Some(cycles_pool) = &mut cycles_pool {
                if cycles_pool.consume_cycles(execution_cycles).is_none() {
                    let cycles = CycleMeter {
                        execution: execution_cycles,
                        r#virtual: context.cycle_meter.r#virtual,
                    };
                    let limit = cycles_pool.limit;

                    if cycles.total() > limit {
                        // Restore cycles pool, because we will not treat this tx as failed tx, it
                        // will be dropped.
                        assert!(org_cycles_pool.is_some());
                        **cycles_pool = org_cycles_pool.unwrap();

                        return Err(TransactionError::ExceededMaxBlockCycles { cycles, limit });
                    } else {
                        return Err(TransactionError::InsufficientPoolCycles { cycles, limit });
                    }
                }
            }

            match maybe_ok {
                Ok(_exit_code) => {
                    exit_code = _exit_code;
                    used_cycles = execution_cycles;
                }
                Err(ckb_vm::error::Error::CyclesExceeded) => {
                    exit_code = INVALID_CYCLES_EXIT_CODE;
                    used_cycles = max_cycles;
                }
                Err(err) => {
                    // Restore cycles pool
                    if let Some((pool, org_pool)) = cycles_pool.as_mut().zip(org_cycles_pool) {
                        **pool = org_pool;
                    }
                    // unexpected VM error
                    return Err(err.into());
                }
            }
            if self.contract_log_config == ContractLogConfig::Verbose || exit_code != 0 {
                let s = std::str::from_utf8(&context.debug_log_buf)
                    .map_err(TransactionError::Utf8Error)?;
                log::debug!("[contract debug]: {}", s);
            }
            log::debug!(
                "[execute tx] VM machine_run time: {}ms, exit code: {} used_cycles: {}",
                t.elapsed().as_millis(),
                exit_code,
                used_cycles
            );
        }
        context.cycle_meter.execution = used_cycles;
        context.exit_code = exit_code;

        Ok(context)
    }

    /// Check withdrawal request signature
    #[instrument(skip_all)]
    pub fn check_withdrawal_signature<S: State + CodeStore>(
        &self,
        state: &S,
        withdrawal: &WithdrawalRequestExtra,
    ) -> Result<(), Error> {
        let raw = withdrawal.request().raw();
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

        let address = state
            .get_registry_address_by_script_hash(
                raw.registry_id().unpack(),
                &account_script_hash.into(),
            )?
            .ok_or(AccountError::RegistryAddressNotFound)?;

        lock_algo.verify_withdrawal(self.rollup_context(), account_script, withdrawal, address)?;

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

        let sender_address = state
            .get_registry_address_by_script_hash(ETH_REGISTRY_ACCOUNT_ID, &script_hash)?
            .ok_or(AccountError::RegistryAddressNotFound)?;

        lock_algo.verify_tx(
            &self.rollup_context,
            sender_address,
            script,
            receiver_script,
            tx.to_owned(),
        )?;
        Ok(())
    }

    /// Apply l2 state transition
    #[instrument(skip_all, fields(block = args.l2block.raw().number().unpack(), deposits_count = args.deposit_info_vec.len()))]
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
        assert_eq!(
            args.l2block.withdrawals().len(),
            args.withdrawals.len(),
            "withdrawal count"
        );

        let mut state = match BlockStateDB::from_store(db, RWConfig::attach_block(block_number)) {
            Ok(state) => state,
            Err(err) => {
                return ApplyBlockResult::Error(err);
            }
        };

        // apply withdrawal to state
        let block_hash = raw_block.hash();
        let block_producer_address = {
            let block_producer: Bytes = block_info.block_producer().unpack();
            match RegistryAddress::from_slice(&block_producer) {
                Some(address) => address,
                None => {
                    return ApplyBlockResult::Error(BlockError::BlockProducerNotExists.into());
                }
            }
        };
        let state_checkpoint_list: Vec<H256> = raw_block.state_checkpoint_list().unpack();

        let mut check_signature_total_ms = 0;
        let mut execute_tx_total_ms = 0;
        let mut apply_state_total_ms = 0;
        let mut withdrawal_receipts = Vec::with_capacity(args.withdrawals.len());
        for (wth_idx, request) in args.withdrawals.into_iter().enumerate() {
            debug_assert_eq!(
                request.request(),
                args.l2block.withdrawals().get(wth_idx).expect("withdrawal")
            );
            debug_assert_eq!(
                {
                    let hash: [u8; 32] = request.request().raw().owner_lock_hash().unpack();
                    hash
                },
                request.owner_lock().hash()
            );
            let now = Instant::now();
            if let Err(error) = self.check_withdrawal_signature(&state, &request) {
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
                &block_producer_address,
                &request.request(),
            ) {
                Ok(receipt) => receipt,
                Err(err) => return ApplyBlockResult::Error(err.into()),
            };
            let expected_checkpoint = state
                .calculate_state_checkpoint()
                .expect("calculate_state_checkpoint");
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

        for req in args.deposit_info_vec.into_iter().map(|i| i.request()) {
            if let Err(err) = state.apply_deposit_request(&self.rollup_context, &req) {
                return ApplyBlockResult::Error(err.into());
            }
        }

        // finalise state
        if let Err(err) = state.finalise() {
            return ApplyBlockResult::Error(err.into());
        }
        let prev_txs_state = match state.calculate_merkle_state() {
            Ok(s) => s,
            Err(err) => {
                return ApplyBlockResult::Error(err.into());
            }
        };

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
        let max_cycles = self
            .rollup_context
            .fork_config
            .max_l2_tx_cycles(block_number);
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
                Err(err) => return ApplyBlockResult::Error(err.into()),
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
                &mut state,
                &block_info,
                &raw_tx,
                Some(max_cycles),
                None,
            ) {
                Ok(run_result) => run_result,
                Err(err) => {
                    let target = build_challenge_target(
                        block_hash.into(),
                        ChallengeTargetType::TxExecution,
                        tx_index as u32,
                    );

                    match err.downcast() {
                        Ok(err) => {
                            return ApplyBlockResult::Challenge {
                                target,
                                error: Error::Transaction(err),
                            };
                        }
                        Err(err) => {
                            log::error!(
                                "Unexpected error {} returned from execute transaction",
                                err
                            );
                            return ApplyBlockResult::Error(err);
                        }
                    }
                }
            };
            execute_tx_total_ms += now.elapsed().as_millis();

            {
                let now = Instant::now();
                // finalise tx state
                if let Err(err) = state.finalise() {
                    return ApplyBlockResult::Error(err.into());
                }
                apply_state_total_ms += now.elapsed().as_millis();
                let expected_checkpoint = state
                    .calculate_state_checkpoint()
                    .expect("calculate_state_checkpoint");
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

                let used_cycles = run_result.cycles.execution;
                let post_state = match state.calculate_merkle_state() {
                    Ok(merkle_state) => merkle_state,
                    Err(err) => return ApplyBlockResult::Error(err.into()),
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
        block_number: u64,
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
                    self.backend_manage
                        .get_backend(block_number, &code_hash.into())
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
    #[instrument(skip_all, fields(block = block_info.number().unpack(), tx_hash = %raw_tx.hash().pack()))]
    pub fn execute_transaction<S: State + CodeStore + JournalDB, C: ChainView>(
        &self,
        chain: &C,
        state: &mut S,
        block_info: &BlockInfo,
        raw_tx: &RawL2Transaction,
        override_max_cycles: Option<u64>,
        cycles_pool: Option<&mut CyclesPool>,
    ) -> Result<RunResult> {
        let account_id = raw_tx.to_id().unpack();
        let script_hash = state.get_script_hash(account_id)?;
        let backend = self
            .load_backend(block_info.number().unpack(), state, &script_hash)
            .ok_or(TransactionError::BackendNotFound { script_hash })?;
        let block_number = block_info.number().unpack();

        let snap = state.snapshot();
        let sender_id: u32 = raw_tx.from_id().unpack();
        let nonce_before = state.get_nonce(sender_id)?;
        state.set_state_tracker(Default::default());

        let max_cycles = override_max_cycles.unwrap_or_else(|| {
            self.rollup_context
                .fork_config
                .max_l2_tx_cycles(block_info.number().unpack())
        });

        let args = MachineRunArgs {
            chain,
            state,
            block_info,
            raw_tx,
            max_cycles,
            backend,
            cycles_pool,
        };

        let run_context = self.machine_run(args).map_err(|err| {
            state.revert(snap).expect("revert");
            err
        })?;

        if run_context.is_success() {
            // check sender's nonce is increased by backends
            let nonce_after = state.get_nonce(sender_id)?;
            if nonce_after <= nonce_before {
                log::error!(
                    "nonce should increased by backends nonce before: {}, nonce after: {}",
                    nonce_before,
                    nonce_after
                );
                return Err(TransactionError::BackendMustIncreaseNonce.into());
            }
        } else {
            // handle failure state, this function will revert to snapshot
            self.handle_failed_transaction(
                state,
                snap,
                nonce_before,
                block_info,
                raw_tx,
                &run_context,
            )?;
        }

        let state_tracker = state.take_state_tracker().unwrap();

        // check write data bytes
        let max_write_data_bytes = self
            .rollup_context
            .fork_config
            .max_write_data_bytes(block_number);
        if let Some(data) = state_tracker
            .write_data()
            .lock()
            .unwrap()
            .values()
            .find(|data| data.len() > max_write_data_bytes)
        {
            return Err(TransactionError::ExceededMaxWriteData {
                max_bytes: max_write_data_bytes,
                used_bytes: data.len(),
            }
            .into());
        }
        // check read data bytes
        let max_read_data_bytes = self
            .rollup_context
            .fork_config
            .max_read_data_bytes(block_number);
        let read_data_bytes: usize = state_tracker
            .read_data()
            .lock()
            .unwrap()
            .values()
            .map(Bytes::len)
            .sum();
        if read_data_bytes > max_read_data_bytes {
            return Err(TransactionError::ExceededMaxReadData {
                max_bytes: max_read_data_bytes,
                used_bytes: read_data_bytes,
            }
            .into());
        }
        let r = RunResult {
            return_data: run_context.return_data,
            logs: state.appended_logs().to_vec(),
            exit_code: run_context.exit_code,
            cycles: run_context.cycle_meter,
            read_data_hashes: state_tracker
                .read_data()
                .lock()
                .unwrap()
                .keys()
                .into_iter()
                .cloned()
                .collect(),
            write_data_hashes: state_tracker
                .write_data()
                .lock()
                .unwrap()
                .keys()
                .into_iter()
                .cloned()
                .collect(),
            debug_log_buf: run_context.debug_log_buf,
        };
        Ok(r)
    }

    pub fn backend_manage(&self) -> &BackendManage {
        &self.backend_manage
    }

    pub fn get_polyjuice_creator_id<S: State + CodeStore>(
        &self,
        state: &S,
    ) -> Result<Option<u32>, TransactionError> {
        if self.polyjuice_creator_id.load_full().is_none() {
            let polyjuice_creator_id =
                get_polyjuice_creator_id(self.rollup_context(), self.backend_manage(), state)?
                    .map(Arc::new);
            self.polyjuice_creator_id.store(polyjuice_creator_id);
        }
        Ok(self.polyjuice_creator_id.load_full().map(|id| *id))
    }

    // Handle failed transaction
    fn handle_failed_transaction<S: State + CodeStore + JournalDB>(
        &self,
        state: &mut S,
        origin_snapshot: usize,
        nonce_before: u32,
        block_info: &BlockInfo,
        raw_tx: &RawL2Transaction,
        run_ctx: &RunContext,
    ) -> Result<()> {
        /// Error code represents EVM internal error
        /// we emit this error from Godwoken side if
        /// Polyjuice failed to generate a system log
        const ERROR_EVM_INTERNAL: i32 = -1;

        let sender_id: u32 = raw_tx.from_id().unpack();

        // revert tx state
        let last_run_result_log = state.appended_logs().last().cloned();
        state.revert(origin_snapshot)?;

        log::debug!("handle failed tx: revert to snapshot {}", origin_snapshot);

        // sender address
        let payer = {
            let script_hash = state.get_script_hash(sender_id)?;
            state
                .get_registry_address_by_script_hash(ETH_REGISTRY_ACCOUNT_ID, &script_hash)?
                .ok_or_else(|| anyhow::Error::from(TransactionError::ScriptHashNotFound))
                .context("failed to find sender's account")?
        };

        // block producer address
        let block_producer = RegistryAddress::from_slice(&block_info.block_producer().raw_data())
            .unwrap_or_default();

        // handle tx fee
        let tx_type = get_tx_type(self.rollup_context(), state, raw_tx)?;
        let typed_tx = match TypedRawTransaction::from_tx(raw_tx.to_owned(), tx_type) {
            Some(tx) => tx,
            None => return Err(TransactionError::UnknownTxType(run_ctx.exit_code).into()),
        };
        let tx_fee = match typed_tx {
            TypedRawTransaction::EthAddrReg(tx) => tx.consumed(),
            TypedRawTransaction::Meta(tx) => tx.consumed(),
            TypedRawTransaction::SimpleUDT(tx) => tx.consumed(),
            TypedRawTransaction::Polyjuice(ref tx) => {
                // push polyjuice system log back to run_result
                let system_log = last_run_result_log
                    .filter(|log| log.service_flag() == GW_LOG_POLYJUICE_SYSTEM.into())
                    .map(Result::<_, TransactionError>::Ok)
                    .unwrap_or_else(|| {
                        // generate a system log for polyjuice tx
                        let polyjuice_tx =
                            crate::typed_transaction::types::PolyjuiceTx::new(raw_tx.to_owned());
                        let p = polyjuice_tx.parser().ok_or(TransactionError::NoCost)?;
                        let gas = p.gas();
                        Ok(generate_polyjuice_system_log(
                            raw_tx.to_id().unpack(),
                            gas,
                            gas,
                            Default::default(),
                            ERROR_EVM_INTERNAL,
                        ))
                    })?;
                let parser = tx.parser().ok_or(TransactionError::NoCost)?;
                let gas_used = match read_polyjuice_gas_used(&system_log) {
                    Some(gas_used) => gas_used,
                    None => {
                        log::warn!(
                            "[gw-generator] failed to parse gas_used, use gas_limit instead"
                        );
                        parser.gas()
                    }
                };
                // append system log
                state.append_log(system_log);
                gw_types::U256::from(gas_used).checked_mul(parser.gas_price().into())
            }
        }
        .ok_or(TransactionError::NoCost)?;

        // pay tx fee
        state
            .pay_fee(&payer, &block_producer, CKB_SUDT_ACCOUNT_ID, tx_fee)
            .map_err(|err| {
                log::error!(
                    "[gw-generator] failed to pay fee for failure tx, err: {}",
                    err
                );
                TransactionError::InsufficientBalance
            })?;

        // Note: update simple UDT total supply
        // This bug cause the ERC-20 pCKB returns wrong total supply. We should fix this logic via a hardfork.
        {
            let raw_key = build_account_key(CKB_SUDT_ACCOUNT_ID, &SUDT_TOTAL_SUPPLY_KEY);
            let mut total_supply = state.get_raw(&raw_key)?.to_u256();
            total_supply = total_supply
                .checked_add(tx_fee)
                .ok_or(TransactionError::InsufficientBalance)?;
            state.update_raw(raw_key, H256::from_u256(total_supply))?;
        }

        // increase sender's nonce
        let nonce = nonce_before
            .checked_add(1)
            .ok_or(TransactionError::NonceOverflow)?;
        state.set_nonce(sender_id, nonce)?;

        Ok(())
    }
}

fn get_block_info(l2block: &RawL2Block) -> BlockInfo {
    BlockInfo::new_builder()
        .block_producer(l2block.block_producer())
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

fn read_polyjuice_gas_used(system_log: &LogItem) -> Option<u64> {
    // read polyjuice system log
    match gw_utils::script_log::parse_log(system_log) {
        Ok(polyjuice_system_log) => {
            if let gw_utils::script_log::GwLog::PolyjuiceSystem { gas_used, .. } =
                polyjuice_system_log
            {
                return Some(gas_used);
            } else {
                log::warn!(
                    "[gw-generator] read_polyjuice_gas_used: can't find polyjuice system log from logs"
                )
            }
        }
        Err(err) => {
            log::warn!("[gw-generator] read_polyjuice_gas_used: an error happend when parsing polyjuice system log, {}", err);
        }
    }
    None
}
