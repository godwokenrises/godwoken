use crate::{
    account_lock_manage::AccountLockManage,
    backend_manage::BackendManage,
    error::{TransactionValidateError, WithdrawalError, WithdrawalErrorWithContext},
    RollupContext,
};
use crate::{
    backend_manage::Backend,
    error::{Error, TransactionError, TransactionErrorWithContext},
    sudt::build_l2_sudt_script,
};
use crate::{error::AccountError, syscalls::L2Syscalls};
use crate::{error::LockAlgorithmError, traits::StateExt};
use gw_common::{
    builtins::CKB_SUDT_ACCOUNT_ID,
    error::Error as StateError,
    h256_ext::H256Ext,
    state::{build_account_field_key, to_short_address, State, GW_ACCOUNT_NONCE},
    H256,
};
use gw_traits::{ChainStore, CodeStore};
use gw_types::{
    core::{ChallengeTargetType, ScriptHashType},
    offchain::RunResult,
    packed::{
        AccountMerkleState, BlockInfo, ChallengeTarget, DepositRequest, L2Block, L2Transaction,
        RawL2Block, RawL2Transaction, TxReceipt, WithdrawalReceipt, WithdrawalRequest,
    },
    prelude::*,
};

use ckb_vm::{
    machine::asm::{AsmCoreMachine, AsmMachine},
    DefaultMachineBuilder,
};

// TODO ensure this value
const MIN_WITHDRAWAL_CAPACITY: u64 = 100_00000000;
// 25 KB
const MAX_WRITE_DATA_BYTES_LIMIT: usize = 25_000;
// 2MB
const MAX_READ_DATA_BYTES_LIMIT: usize = 1024 * 1024 * 2;

pub struct StateTransitionArgs {
    pub l2block: L2Block,
    pub deposit_requests: Vec<DepositRequest>,
}

pub struct StateTransitionResult {
    pub withdrawal_receipts: Vec<WithdrawalReceipt>,
    pub tx_receipts: Vec<TxReceipt>,
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

    /// Verify withdrawal request
    /// Notice this function do not perform signature check
    pub fn verify_withdrawal_request<S: State + CodeStore>(
        &self,
        state: &S,
        withdrawal_request: &WithdrawalRequest,
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

        // check capacity
        if capacity < MIN_WITHDRAWAL_CAPACITY {
            return Err(AccountError::InsufficientCapacity {
                expected: MIN_WITHDRAWAL_CAPACITY,
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

        // check fees if it isn't been cheked yet
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
            lock_algo.verify_tx(&self.rollup_context, script, receiver_script, &tx)?;
        if !valid_signature {
            return Err(LockAlgorithmError::InvalidSignature.into());
        }
        Ok(())
    }

    /// Apply l2 state transition
    pub fn verify_and_apply_state_transition<S: State + CodeStore, C: ChainStore>(
        &self,
        chain: &C,
        state: &mut S,
        args: StateTransitionArgs,
    ) -> Result<StateTransitionResult, Error> {
        let raw_block = args.l2block.raw();
        let block_info = get_block_info(&raw_block);

        // apply withdrawal to state
        let withdrawal_requests: Vec<_> = args.l2block.withdrawals().into_iter().collect();
        let block_hash = raw_block.hash();
        let block_producer_id: u32 = block_info.block_producer_id().unpack();

        let mut withdrawal_receipts = Vec::with_capacity(withdrawal_requests.len());
        for (wth_idx, request) in withdrawal_requests.into_iter().enumerate() {
            if let Err(err) = self.check_withdrawal_request_signature(state, &request) {
                return match err {
                    Error::Withdrawal(err) => {
                        let target = build_challenge_target(
                            block_hash.into(),
                            ChallengeTargetType::Withdrawal,
                            wth_idx as u32,
                        );
                        Err(WithdrawalErrorWithContext::new(target, err).into())
                    }
                    _ => Err(err),
                };
            }

            let withdrawal_receipt = state.apply_withdrawal_request(
                &self.rollup_context,
                block_producer_id,
                &request,
            )?;
            withdrawal_receipts.push(withdrawal_receipt);
        }
        // apply deposition to state
        state.apply_deposit_requests(&self.rollup_context, &args.deposit_requests)?;

        // handle transactions
        let mut tx_receipts = Vec::with_capacity(args.l2block.transactions().len());
        for (tx_index, tx) in args.l2block.transactions().into_iter().enumerate() {
            if let Err(err) = self.check_transaction_signature(state, &tx) {
                return match err {
                    TransactionValidateError::Transaction(tx_err) => {
                        let target = build_challenge_target(
                            block_hash.into(),
                            ChallengeTargetType::TxSignature,
                            tx_index as u32,
                        );
                        Err(TransactionErrorWithContext::new(target, tx_err).into())
                    }
                    TransactionValidateError::Account(err) => Err(Error::Account(err)),
                    TransactionValidateError::State(err) => Err(Error::State(err)),
                    TransactionValidateError::Unlock(err) => Err(Error::Unlock(err)),
                };
            }

            let raw_tx = tx.raw();
            // check nonce
            let expected_nonce = state.get_nonce(raw_tx.from_id().unpack())?;
            let actual_nonce: u32 = raw_tx.nonce().unpack();
            if actual_nonce != expected_nonce {
                return Err(TransactionErrorWithContext::new(
                    build_challenge_target(
                        block_hash.into(),
                        ChallengeTargetType::TxSignature,
                        tx_index as u32,
                    ),
                    TransactionError::Nonce {
                        expected: expected_nonce,
                        actual: actual_nonce,
                    },
                )
                .into());
            }
            // build call context
            // NOTICE users only allowed to send HandleMessage CallType txs
            let run_result = match self.execute_transaction(chain, state, &block_info, &raw_tx) {
                Ok(run_result) => run_result,
                Err(err) => {
                    return Err(TransactionErrorWithContext::new(
                        build_challenge_target(
                            block_hash.into(),
                            ChallengeTargetType::TxExecution,
                            tx_index as u32,
                        ),
                        err,
                    )
                    .into());
                }
            };
            state.apply_run_result(&run_result)?;

            let post_state = {
                let account_root = state.calculate_root()?;
                let account_count = state.get_account_count()?;
                AccountMerkleState::new_builder()
                    .merkle_root(account_root.pack())
                    .count(account_count.pack())
                    .build()
            };
            let tx_receipt = TxReceipt::new_builder()
                .tx_witness_hash(tx.witness_hash().pack())
                .post_state(post_state)
                .read_data_hashes(
                    run_result
                        .read_data
                        .into_iter()
                        .map(|(hash, _)| hash.pack())
                        .collect::<Vec<_>>()
                        .pack(),
                )
                .logs(run_result.logs.pack())
                .build();
            tx_receipts.push(tx_receipt);
        }

        let result = StateTransitionResult {
            withdrawal_receipts,
            tx_receipts,
        };

        Ok(result)
    }

    fn load_backend<S: State + CodeStore>(&self, state: &S, script_hash: &H256) -> Option<Backend> {
        log::debug!("load_backend for script_hash: {:?}", script_hash);
        state
            .get_script(&script_hash)
            .and_then(|script| {
                // only accept type script hash type for now
                if script.hash_type() == ScriptHashType::Type.into() {
                    let code_hash: [u8; 32] = script.code_hash().unpack();
                    log::debug!("load_backend by code_hash: {:?}", code_hash);
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
    pub fn execute_transaction<S: State + CodeStore, C: ChainStore>(
        &self,
        chain: &C,
        state: &S,
        block_info: &BlockInfo,
        raw_tx: &RawL2Transaction,
    ) -> Result<RunResult, TransactionError> {
        let mut run_result = RunResult::default();
        {
            let core_machine = Box::<AsmCoreMachine>::default();
            let machine_builder =
                DefaultMachineBuilder::new(core_machine).syscall(Box::new(L2Syscalls {
                    chain,
                    state,
                    block_info,
                    raw_tx,
                    rollup_context: &self.rollup_context,
                    account_lock_manage: &self.account_lock_manage,
                    result: &mut run_result,
                    code_store: state,
                }));
            let mut machine = AsmMachine::new(machine_builder.build(), None);
            let account_id = raw_tx.to_id().unpack();
            let script_hash = state.get_script_hash(account_id)?;
            let backend = self
                .load_backend(state, &script_hash)
                .ok_or(TransactionError::BackendNotFound { script_hash })?;
            machine.load_program(&backend.generator, &[])?;
            let code = machine.run()?;
            if code != 0 {
                return Err(TransactionError::InvalidExitCode(code));
            }
        }
        // set nonce
        let sender_id: u32 = raw_tx.from_id().unpack();
        let nonce = state.get_nonce(sender_id)?;
        let nonce_raw_key = build_account_field_key(sender_id, GW_ACCOUNT_NONCE);
        if run_result.read_values.get(&nonce_raw_key).is_none() {
            run_result
                .read_values
                .insert(nonce_raw_key, H256::from_u32(nonce));
        }
        // increase nonce
        run_result
            .write_values
            .insert(nonce_raw_key, H256::from_u32(nonce + 1));

        // check write data bytes
        let write_data_bytes: usize = run_result.write_data.values().map(|data| data.len()).sum();
        if write_data_bytes > MAX_WRITE_DATA_BYTES_LIMIT {
            return Err(TransactionError::ExceededMaxWriteData {
                max_bytes: MAX_WRITE_DATA_BYTES_LIMIT,
                used_bytes: write_data_bytes,
            });
        }
        // check read data bytes
        let read_data_bytes: usize = run_result.read_data.values().sum();
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
