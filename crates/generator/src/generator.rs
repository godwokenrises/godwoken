use std::{collections::HashSet, sync::atomic::Ordering::SeqCst, time::Instant};

use crate::{
    account_lock_manage::AccountLockManage,
    backend_manage::BackendManage,
    constants::{L2TX_MAX_CYCLES, MAX_READ_DATA_BYTES_LIMIT, MAX_WRITE_DATA_BYTES_LIMIT},
    error::{BlockError, TransactionValidateError, WithdrawalError},
    run_result_state::RunResultState,
    syscalls::redir_log::RedirLogHandler,
    typed_transaction::types::TypedRawTransaction,
    types::vm::VMVersion,
    utils::get_tx_type,
    vm_cost_model::instruction_cycles,
};
use crate::{
    backend_manage::Backend,
    error::{Error, TransactionError},
};
use crate::{error::AccountError, syscalls::L2Syscalls};
use crate::{error::LockAlgorithmError, traits::StateExt};
use gw_ckb_hardfork::GLOBAL_VM_VERSION;
use gw_common::{
    builtins::{CKB_SUDT_ACCOUNT_ID, ETH_REGISTRY_ACCOUNT_ID},
    error::Error as StateError,
    h256_ext::H256Ext,
    registry_address::RegistryAddress,
    state::{
        build_account_field_key, build_account_key, build_sudt_key, State, GW_ACCOUNT_NONCE_TYPE,
        SUDT_KEY_FLAG_BALANCE, SUDT_TOTAL_SUPPLY_KEY,
    },
    H256,
};
use gw_config::{ContractLogConfig, SyscallCyclesConfig};
use gw_store::{state::state_db::StateContext, transaction::StoreTransaction};
use gw_traits::{ChainView, CodeStore};
use gw_types::{
    bytes::Bytes,
    core::{ChallengeTargetType, ScriptHashType},
    offchain::{RollupContext, RunResult, RunResultCycles},
    packed::{
        AccountMerkleState, BlockInfo, ChallengeTarget, DepositRequest, L2Block, L2Transaction,
        RawL2Block, RawL2Transaction, TxReceipt, WithdrawalReceipt, WithdrawalRequestExtra,
    },
    prelude::*,
};

use ckb_vm::{DefaultMachineBuilder, SupportMachine};

#[cfg(not(has_asm))]
use ckb_vm::TraceMachine;
use gw_utils::script_log::{generate_polyjuice_system_log, GW_LOG_POLYJUICE_SYSTEM};
use tracing::instrument;

pub struct ApplyBlockArgs {
    pub l2block: L2Block,
    pub deposit_requests: Vec<DepositRequest>,
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
    Error(Error),
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

    pub fn none<'a>() -> Option<&'a mut CyclesPool> {
        None
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

    pub fn checked_sub_cycles(&mut self, cycles: u64) -> Option<u64> {
        let opt_available_cycles = self.available_cycles.checked_sub(cycles);
        self.available_cycles = opt_available_cycles.unwrap_or(0);

        opt_available_cycles
    }
}

pub struct MachineRunArgs<'a, C, S> {
    chain: &'a C,
    state: &'a S,
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
    redir_log_handler: RedirLogHandler,
}

impl Generator {
    pub fn new(
        backend_manage: BackendManage,
        account_lock_manage: AccountLockManage,
        rollup_context: RollupContext,
        contract_log_config: ContractLogConfig,
    ) -> Self {
        let redir_log_handler = RedirLogHandler::new(contract_log_config);
        Generator {
            backend_manage,
            account_lock_manage,
            rollup_context,
            redir_log_handler,
        }
    }

    pub fn rollup_context(&self) -> &RollupContext {
        &self.rollup_context
    }

    pub fn account_lock_manage(&self) -> &AccountLockManage {
        &self.account_lock_manage
    }

    #[instrument(skip_all, fields(backend = ?args.backend.backend_type))]
    fn machine_run<S: State + CodeStore, C: ChainView>(
        &self,
        args: MachineRunArgs<'_, C, S>,
    ) -> Result<RunResult, TransactionError> {
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

        self.redir_log_handler.start(raw_tx);
        let mut run_result = RunResult::default();
        let used_cycles;
        let exit_code;
        let org_cycles_pool = cycles_pool.as_mut().map(|p| p.clone());
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
                    redir_log_handler: &self.redir_log_handler,
                    cycles_pool: &mut cycles_pool,
                }))
                .instruction_cycle_func(Box::new(instruction_cycles));
            let default_machine = machine_builder.build();

            #[cfg(has_asm)]
            let aot_code_opt = self
                .backend_manage
                .get_aot_code(&backend.checksum.generator, global_vm_version);
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
                if cycles_pool.checked_sub_cycles(execution_cycles).is_none() {
                    let cycles = RunResultCycles {
                        execution: execution_cycles,
                        r#virtual: run_result.cycles.r#virtual,
                    };
                    let limit = cycles_pool.limit;

                    if cycles.total() > limit {
                        // Restore cycles pool, because we will not treat this tx as failed tx
                        assert!(org_cycles_pool.is_some());
                        **cycles_pool = org_cycles_pool.unwrap();

                        return Err(TransactionError::ExceededBlockMaxCycles { cycles, limit });
                    } else {
                        return Err(TransactionError::BlockCyclesLimitReached { cycles, limit });
                    }
                }
            }

            match maybe_ok {
                Ok(_exit_code) => {
                    exit_code = _exit_code;
                    used_cycles = execution_cycles;
                }
                Err(ckb_vm::error::Error::InvalidCycles) => {
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
            self.redir_log_handler.flush(exit_code);
            log::debug!(
                "[execute tx] VM machine_run time: {}ms, exit code: {} used_cycles: {}",
                t.elapsed().as_millis(),
                exit_code,
                used_cycles
            );
        }
        run_result.cycles.execution = used_cycles;
        run_result.exit_code = exit_code;

        Ok(run_result)
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

        lock_algo.verify_withdrawal(account_script, withdrawal, address)?;

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
        assert_eq!(
            args.l2block.withdrawals().len(),
            args.withdrawals.len(),
            "withdrawal count"
        );

        let mut state = match db.state_tree(StateContext::AttachBlock(block_number)) {
            Ok(state) => state,
            Err(err) => {
                log::error!("next state {}", err);
                return ApplyBlockResult::Error(Error::State(StateError::Store));
            }
        };

        // apply withdrawal to state
        let block_hash = raw_block.hash();
        let block_producer_address = {
            let block_producer: Bytes = block_info.block_producer().unpack();
            match RegistryAddress::from_slice(&block_producer) {
                Some(address) => address,
                None => {
                    return ApplyBlockResult::Error(Error::Block(
                        BlockError::BlockProducerNotExists,
                    ));
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
                Err(err) => return ApplyBlockResult::Error(err),
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
                CyclesPool::none(),
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
                if let Err(err) = state.apply_run_result(&run_result.write) {
                    return ApplyBlockResult::Error(err);
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
    #[instrument(skip_all)]
    pub fn execute_transaction<'a, S: State + CodeStore, C: ChainView>(
        &'a self,
        chain: &'a C,
        state: &'a S,
        block_info: &'a BlockInfo,
        raw_tx: &'a RawL2Transaction,
        max_cycles: u64,
        cycles_pool: Option<&'a mut CyclesPool>,
    ) -> Result<RunResult, TransactionError> {
        let run_result = self.unchecked_execute_transaction(
            chain,
            state,
            block_info,
            raw_tx,
            max_cycles,
            cycles_pool,
        )?;
        Ok(run_result)
    }

    /// execute a layer2 tx, doesn't check exit code
    #[instrument(skip_all, fields(block = block_info.number().unpack(), tx_hash = %raw_tx.hash().pack()))]
    pub fn unchecked_execute_transaction<'a, S: State + CodeStore, C: ChainView>(
        &'a self,
        chain: &'a C,
        state: &'a S,
        block_info: &'a BlockInfo,
        raw_tx: &'a RawL2Transaction,
        max_cycles: u64,
        cycles_pool: Option<&'a mut CyclesPool>,
    ) -> Result<RunResult, TransactionError> {
        let account_id = raw_tx.to_id().unpack();
        let script_hash = state.get_script_hash(account_id)?;
        let backend = self
            .load_backend(block_info.number().unpack(), state, &script_hash)
            .ok_or(TransactionError::BackendNotFound { script_hash })?;

        let args = MachineRunArgs {
            chain,
            state,
            block_info,
            raw_tx,
            max_cycles,
            backend,
            cycles_pool,
        };

        let run_result: RunResult = self.machine_run(args)?;
        self.handle_run_result(state, block_info, raw_tx, run_result)
    }

    pub fn backend_manage(&self) -> &BackendManage {
        &self.backend_manage
    }

    // check and handle run_result before return
    fn handle_run_result<S: State + CodeStore>(
        &self,
        state: &S,
        block_info: &BlockInfo,
        raw_tx: &RawL2Transaction,
        mut run_result: RunResult,
    ) -> Result<RunResult, TransactionError> {
        let sender_id: u32 = raw_tx.from_id().unpack();
        let nonce_raw_key = build_account_field_key(sender_id, GW_ACCOUNT_NONCE_TYPE);
        let nonce_before = state.get_nonce(sender_id)?;

        if 0 == run_result.exit_code {
            // check sender's nonce is increased by backends
            let nonce_after = {
                let value = run_result
                    .write
                    .write_values
                    .get(&nonce_raw_key)
                    .ok_or(TransactionError::BackendMustIncreaseNonce)?;
                value.to_u32()
            };
            if nonce_after <= nonce_before {
                log::error!(
                    "nonce should increased by backends nonce before: {}, nonce after: {}",
                    nonce_before,
                    nonce_after
                );
                return Err(TransactionError::BackendMustIncreaseNonce);
            }
        } else {
            // revert tx
            let last_run_result_log = run_result.write.logs.pop();
            run_result.revert_write();

            // sender address
            let payer = {
                let script_hash = state.get_script_hash(sender_id)?;
                state
                    .get_registry_address_by_script_hash(ETH_REGISTRY_ACCOUNT_ID, &script_hash)?
                    .ok_or(TransactionError::ScriptHashNotFound)?
            };

            // block producer address
            let block_producer =
                RegistryAddress::from_slice(&block_info.block_producer().raw_data())
                    .unwrap_or_default();

            // add charging fee key-values into run result
            {
                let sender_sudt_key = {
                    let sudt_key = build_sudt_key(SUDT_KEY_FLAG_BALANCE, &payer);
                    build_account_key(CKB_SUDT_ACCOUNT_ID, &sudt_key)
                };
                let block_producer_sudt_key = {
                    let sudt_key = build_sudt_key(SUDT_KEY_FLAG_BALANCE, &block_producer);
                    build_account_key(CKB_SUDT_ACCOUNT_ID, &sudt_key)
                };
                let total_supply_key =
                    build_account_key(CKB_SUDT_ACCOUNT_ID, &SUDT_TOTAL_SUPPLY_KEY);
                for raw_key in [sender_sudt_key, block_producer_sudt_key, total_supply_key] {
                    let raw_value = state.get_raw(&raw_key)?;
                    run_result.read_values.entry(raw_key).or_insert(raw_value);
                }
            }

            // handle tx fee
            let tx_type = get_tx_type(self.rollup_context(), state, raw_tx)?;
            let typed_tx = TypedRawTransaction::from_tx(raw_tx.to_owned(), tx_type)
                .expect("Unknown type of tx");
            let tx_fee = match typed_tx {
                TypedRawTransaction::EthAddrReg(tx) => tx.consumed(),
                TypedRawTransaction::Meta(tx) => tx.consumed(),
                TypedRawTransaction::SimpleUDT(tx) => tx.consumed(),
                TypedRawTransaction::Polyjuice(ref tx) => {
                    // push polyjuice system log back to run_result
                    if let Some(log) = last_run_result_log
                        .filter(|log| log.service_flag() == GW_LOG_POLYJUICE_SYSTEM.into())
                    {
                        run_result.write.logs.push(log);
                    } else {
                        // generate a system log for polyjuice tx
                        let polyjuice_tx =
                            crate::typed_transaction::types::PolyjuiceTx::new(raw_tx.to_owned());
                        let p = polyjuice_tx.parser().ok_or(TransactionError::NoCost)?;
                        let gas = p.gas();
                        run_result.write.logs.push(generate_polyjuice_system_log(
                            raw_tx.to_id().unpack(),
                            gas,
                            gas,
                            Default::default(),
                            0,
                        ));
                    }
                    let parser = tx.parser().ok_or(TransactionError::NoCost)?;
                    let gas_used = match read_polyjuice_gas_used(&run_result) {
                        Some(gas_used) => gas_used,
                        None => {
                            log::warn!(
                                "[gw-generator] failed to parse gas_used, use gas_limit instead"
                            );
                            parser.gas()
                        }
                    };
                    gw_types::U256::from(gas_used).checked_mul(parser.gas_price().into())
                }
            }
            .ok_or(TransactionError::NoCost)?;

            let mut run_result_state = RunResultState(&mut run_result);

            run_result_state
                .pay_fee(&payer, &block_producer, CKB_SUDT_ACCOUNT_ID, tx_fee)
                .map_err(|err| {
                    log::error!(
                        "[gw-generator] failed to pay fee for failure tx, err: {}",
                        err
                    );
                    TransactionError::InsufficientBalance
                })?;

            // increase sender's nonce
            let nonce = nonce_before
                .checked_add(1)
                .ok_or(TransactionError::NonceOverflow)?;
            run_result
                .write
                .write_values
                .insert(nonce_raw_key, H256::from_u32(nonce));
        }

        // check write data bytes
        if let Some(data) = run_result
            .write
            .write_data
            .values()
            .find(|data| data.len() > MAX_WRITE_DATA_BYTES_LIMIT)
        {
            return Err(TransactionError::ExceededMaxWriteData {
                max_bytes: MAX_WRITE_DATA_BYTES_LIMIT,
                used_bytes: data.len(),
            });
        }
        // check read data bytes
        let read_data_bytes: usize = run_result.read_data.values().map(Bytes::len).sum();
        if read_data_bytes > MAX_READ_DATA_BYTES_LIMIT {
            return Err(TransactionError::ExceededMaxReadData {
                max_bytes: MAX_READ_DATA_BYTES_LIMIT,
                used_bytes: read_data_bytes,
            });
        }

        Ok(run_result)
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

fn read_polyjuice_gas_used(run_result: &RunResult) -> Option<u64> {
    // read polyjuice system log
    match run_result
        .write
        .logs
        .iter()
        .find(|item| u8::from(item.service_flag()) == gw_utils::script_log::GW_LOG_POLYJUICE_SYSTEM)
        .map(gw_utils::script_log::parse_log)
    {
        Some(Ok(polyjuice_system_log)) => {
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
        Some(Err(err)) => {
            log::warn!("[gw-generator] read_polyjuice_gas_used: an error happend when parsing polyjuice system log, {}", err);
        }
        None => {
            log::warn!("[gw-generator] read_polyjuice_gas_used: Can't find polyjuice system log");
        }
    }
    None
}
