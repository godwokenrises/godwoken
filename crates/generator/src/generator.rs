use std::{
    collections::{HashMap, HashSet},
    time::Instant,
};

use crate::{
    account_lock_manage::AccountLockManage,
    backend_manage::BackendManage,
    constants::{MAX_READ_DATA_BYTES_LIMIT, MAX_WRITE_DATA_BYTES_LIMIT},
    erc20_creator_allowlist::SUDTProxyAccountAllowlist,
    error::{BlockError, TransactionValidateError, WithdrawalError},
    vm_cost_model::instruction_cycles,
    Machine, VMVersion,
};
use crate::{
    backend_manage::Backend,
    error::{Error, TransactionError},
    sudt::build_l2_sudt_script,
};
use crate::{error::AccountError, syscalls::L2Syscalls};
use crate::{error::LockAlgorithmError, traits::StateExt};
use gw_ckb_hardfork::GLOBAL_VM_VERSION;
use gw_common::{
    builtins::CKB_SUDT_ACCOUNT_ID,
    error::Error as StateError,
    h256_ext::H256Ext,
    merkle_utils::calculate_state_checkpoint,
    state::{build_account_field_key, to_short_address, State, GW_ACCOUNT_NONCE_TYPE},
    H256,
};
use gw_config::RPCConfig;
use gw_store::{state::state_db::StateContext, transaction::StoreTransaction};
use gw_traits::{ChainStore, CodeStore};
use gw_tx_filter::polyjuice_contract_creator_allowlist::PolyjuiceContractCreatorAllowList;
use gw_types::{
    bytes::Bytes,
    core::{ChallengeTargetType, ScriptHashType},
    offchain::{RollupContext, RunResult},
    packed::{
        AccountMerkleState, BlockInfo, CellOutput, ChallengeTarget, DepositRequest, L2Block,
        L2Transaction, RawL2Block, RawL2Transaction, Script, TxReceipt, WithdrawalLockArgs,
        WithdrawalReceipt, WithdrawalRequest,
    },
    prelude::*,
};

use ckb_vm::{DefaultMachineBuilder, SupportMachine};

#[cfg(has_asm)]
use ckb_vm::machine::asm::AsmMachine;

#[cfg(not(has_asm))]
use ckb_vm::TraceMachine;

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

pub struct Generator {
    backend_manage: BackendManage,
    account_lock_manage: AccountLockManage,
    rollup_context: RollupContext,
    sudt_proxy_account_whitelist: SUDTProxyAccountAllowlist,
    polyjuice_contract_creator_allowlist: Option<PolyjuiceContractCreatorAllowList>,
    default_l2tx_max_cycles: u64,
}

impl Generator {
    pub fn new(
        backend_manage: BackendManage,
        account_lock_manage: AccountLockManage,
        rollup_context: RollupContext,
        rpc_config: RPCConfig,
        default_l2tx_max_cycles: u64,
    ) -> Self {
        let polyjuice_contract_creator_allowlist =
            PolyjuiceContractCreatorAllowList::from_rpc_config(&rpc_config);

        let sudt_proxy_account_whitelist = SUDTProxyAccountAllowlist::new(
            rpc_config.allowed_sudt_proxy_creator_account_id,
            rpc_config
                .sudt_proxy_code_hashes
                .into_iter()
                .map(|hash| hash.0.into())
                .collect(),
        );

        Generator {
            backend_manage,
            account_lock_manage,
            rollup_context,
            sudt_proxy_account_whitelist,
            polyjuice_contract_creator_allowlist,
            default_l2tx_max_cycles,
        }
    }

    pub fn rollup_context(&self) -> &RollupContext {
        &self.rollup_context
    }

    pub fn account_lock_manage(&self) -> &AccountLockManage {
        &self.account_lock_manage
    }

    fn build_machine<'a, S: State + CodeStore, C: ChainStore>(
        &'a self,
        run_result: &'a mut RunResult,
        chain: &'a C,
        state: &'a S,
        block_info: &'a BlockInfo,
        raw_tx: &'a RawL2Transaction,
        max_cycles: u64,
    ) -> Machine<'a> {
        let global_vm_version = smol::block_on(async { *GLOBAL_VM_VERSION.lock().await });
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
                result: run_result,
                code_store: state,
            }))
            .instruction_cycle_func(Box::new(instruction_cycles));
        let default_machine = machine_builder.build();

        #[cfg(has_asm)]
        let machine = AsmMachine::new(default_machine, None);
        #[cfg(not(has_asm))]
        let machine = TraceMachine::new(default_machine);

        machine
    }

    /// Verify withdrawal request
    /// Notice this function do not perform signature check
    pub fn verify_withdrawal_request<S: State + CodeStore>(
        &self,
        state: &S,
        withdrawal_request: &WithdrawalRequest,
        asset_script: Option<Script>,
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
        if let Err(min_capacity) = Self::build_withdrawal_cell_output(
            rollup_context,
            withdrawal_request,
            &H256::one(),
            1,
            asset_script,
        ) {
            return Err(AccountError::InsufficientCapacity {
                expected: min_capacity,
                actual: capacity,
            }
            .into());
        }

        // find user account
        let id = state
            .get_account_id_by_script_hash(&account_script_hash)?
            .ok_or(AccountError::UnknownAccount)?; // find Simple UDT account

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
        Ok(())
    }

    /// Check withdrawal request signature
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
    pub fn verify_and_apply_block<C: ChainStore>(
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

        let mut withdrawal_receipts = Vec::with_capacity(withdrawal_requests.len());
        for (wth_idx, request) in withdrawal_requests.into_iter().enumerate() {
            if let Err(error) = self.check_withdrawal_request_signature(&state, &request) {
                let target = build_challenge_target(
                    block_hash.into(),
                    ChallengeTargetType::Withdrawal,
                    wth_idx as u32,
                );

                return ApplyBlockResult::Challenge { target, error };
            }

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
            let run_result = match self.execute_transaction_with_default_max_cycles(
                chain,
                &state,
                &block_info,
                &raw_tx,
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

            {
                if let Err(err) = state.apply_run_result(&run_result) {
                    return ApplyBlockResult::Error(err);
                }
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

        ApplyBlockResult::Success {
            withdrawal_receipts,
            prev_txs_state,
            tx_receipts,
            offchain_used_cycles,
        }
    }

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

    /// execute a l2tx with default_l2tx_max_cycles
    pub fn execute_transaction_with_default_max_cycles<S: State + CodeStore, C: ChainStore>(
        &self,
        chain: &C,
        state: &S,
        block_info: &BlockInfo,
        raw_tx: &RawL2Transaction,
    ) -> Result<RunResult, TransactionError> {
        let run_result = self.unchecked_execute_transaction(
            chain,
            state,
            block_info,
            raw_tx,
            self.default_l2tx_max_cycles,
        )?;
        if 0 != run_result.exit_code {
            return Err(TransactionError::InvalidExitCode(run_result.exit_code));
        }

        Ok(run_result)
    }

    /// execute a layer2 tx
    pub fn execute_transaction<S: State + CodeStore, C: ChainStore>(
        &self,
        chain: &C,
        state: &S,
        block_info: &BlockInfo,
        raw_tx: &RawL2Transaction,
        max_cycles: u64,
    ) -> Result<RunResult, TransactionError> {
        let run_result =
            self.unchecked_execute_transaction(chain, state, block_info, raw_tx, max_cycles)?;
        if 0 != run_result.exit_code {
            return Err(TransactionError::InvalidExitCode(run_result.exit_code));
        }

        Ok(run_result)
    }

    /// execute a layer2 tx, doesn't check exit code
    pub fn unchecked_execute_transaction<S: State + CodeStore, C: ChainStore>(
        &self,
        chain: &C,
        state: &S,
        block_info: &BlockInfo,
        raw_tx: &RawL2Transaction,
        max_cycles: u64,
    ) -> Result<RunResult, TransactionError> {
        if let Some(polyjuice_contract_creator_allowlist) =
            self.polyjuice_contract_creator_allowlist.as_ref()
        {
            use gw_tx_filter::polyjuice_contract_creator_allowlist::Error;
            match polyjuice_contract_creator_allowlist.validate_with_state(state, raw_tx) {
                Ok(_) => (),
                Err(Error::Common(err)) => return Err(TransactionError::from(err)),
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

        let mut run_result = RunResult::default();
        let used_cycles;
        let exit_code;
        {
            let mut machine = self.build_machine(
                &mut run_result,
                chain,
                state,
                block_info,
                raw_tx,
                max_cycles,
            );
            let account_id = raw_tx.to_id().unpack();
            let script_hash = state.get_script_hash(account_id)?;
            let backend = self
                .load_backend(state, &script_hash)
                .ok_or(TransactionError::BackendNotFound { script_hash })?;
            machine.load_program(&backend.generator, &[])?;
            let t = Instant::now();
            exit_code = machine.run()?;
            log::debug!(
                "[execute tx] VM run time: {}ms, exit code: {}",
                t.elapsed().as_millis(),
                exit_code
            );
            used_cycles = machine.machine.cycles();
        }
        // record used cycles
        log::debug!("run_result.used_cycles = {}", used_cycles);
        run_result.used_cycles = used_cycles;
        run_result.exit_code = exit_code;

        if 0 == exit_code {
            // check nonce is increased by backends
            let nonce_after_execution = {
                let nonce_raw_key = build_account_field_key(sender_id, GW_ACCOUNT_NONCE_TYPE);
                let value = run_result
                    .write_values
                    .get(&nonce_raw_key)
                    .expect("Backend must update nonce");
                value.to_u32()
            };
            assert!(
                nonce_after_execution > nonce_before_execution,
                "nonce should increased by backends"
            );
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
        if self
            .sudt_proxy_account_whitelist
            .validate(&run_result, from_id)
        {
            Ok(run_result)
        } else {
            Err(TransactionError::InvalidSUDTProxyCreatorAccount {
                account_id: from_id,
            })
        }
    }

    pub fn build_withdrawal_cell_output(
        rollup_context: &RollupContext,
        req: &WithdrawalRequest,
        block_hash: &H256,
        block_number: u64,
        asset_script: Option<Script>,
    ) -> Result<(CellOutput, Bytes), u128> {
        let withdrawal_capacity: u64 = req.raw().capacity().unpack();
        let lock_args: Bytes = {
            let withdrawal_lock_args = WithdrawalLockArgs::new_builder()
                .account_script_hash(req.raw().account_script_hash())
                .withdrawal_block_hash(Into::<[u8; 32]>::into(*block_hash).pack())
                .withdrawal_block_number(block_number.pack())
                .sudt_script_hash(req.raw().sudt_script_hash())
                .sell_amount(req.raw().sell_amount())
                .sell_capacity(withdrawal_capacity.pack())
                .owner_lock_hash(req.raw().owner_lock_hash())
                .payment_lock_hash(req.raw().payment_lock_hash())
                .build();

            let rollup_type_hash = rollup_context.rollup_script_hash.as_slice().iter();
            rollup_type_hash
                .chain(withdrawal_lock_args.as_slice().iter())
                .cloned()
                .collect()
        };

        let lock = Script::new_builder()
            .code_hash(rollup_context.rollup_config.withdrawal_script_type_hash())
            .hash_type(ScriptHashType::Type.into())
            .args(lock_args.pack())
            .build();

        let (type_, data) = match asset_script {
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
                return Err(min_capacity as u128)
            }
            Err(err) => {
                log::debug!("calculate withdrawal capacity {}", err); // Overflow
                return Err(u64::MAX as u128 + 1);
            }
            _ => (),
        }

        Ok((output, data))
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
