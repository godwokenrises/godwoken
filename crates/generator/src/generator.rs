use crate::{account_lock_manage::AccountLockManage, backend_manage::BackendManage};
use crate::{
    backend_manage::Backend,
    error::{Error, TransactionError, TransactionErrorWithContext},
};
use crate::{
    error::LockAlgorithmError,
    traits::{CodeStore, StateExt},
};
use crate::{
    error::ValidateError,
    syscalls::{L2Syscalls, RunResult},
};
use gw_common::{
    error::Error as StateError,
    h256_ext::H256Ext,
    state::{build_account_field_key, State, GW_ACCOUNT_NONCE},
    H256,
};
use gw_types::{
    core::ScriptHashType,
    packed::{
        BlockInfo, DepositionRequest, L2Block, RawL2Block, RawL2Transaction, StartChallenge,
        WithdrawalRequest,
    },
    prelude::*,
};

use ckb_vm::{
    machine::asm::{AsmCoreMachine, AsmMachine},
    DefaultMachineBuilder,
};

// TODO ensure this value
const MIN_WITHDRAWAL_CAPACITY: u64 = 100_0000_0000;

pub struct StateTransitionArgs {
    pub l2block: L2Block,
    pub deposition_requests: Vec<DepositionRequest>,
}

pub struct Generator {
    backend_manage: BackendManage,
    account_lock_manage: AccountLockManage,
}

impl Generator {
    pub fn new(backend_manage: BackendManage, account_lock_manage: AccountLockManage) -> Self {
        Generator {
            backend_manage,
            account_lock_manage,
        }
    }

    pub fn verify_withdrawal_request<S: State + CodeStore>(
        &self,
        state: &S,
        withdrawal_request: &WithdrawalRequest,
    ) -> Result<(), Error> {
        let raw = withdrawal_request.raw();
        let account_script_hash: [u8; 32] = raw.account_script_hash().unpack();
        let sudt_script_hash: [u8; 32] = raw.sudt_script_hash().unpack();
        let amount: u128 = raw.amount().unpack();
        let capacity: u64 = raw.capacity().unpack();

        // check capacity
        if capacity < MIN_WITHDRAWAL_CAPACITY {
            return Err(ValidateError::InsufficientCapacity {
                expected: MIN_WITHDRAWAL_CAPACITY,
                actual: capacity,
            }
            .into());
        }

        // check signature
        let account_script = state
            .get_script(&account_script_hash.into())
            .ok_or(StateError::MissingKey)?;
        let lock_code_hash: [u8; 32] = account_script.code_hash().unpack();
        let lock_algo = self
            .account_lock_manage
            .get_lock_algorithm(&lock_code_hash.into())
            .ok_or(ValidateError::UnknownAccountLockScript)?;

        let message = raw.hash().into();
        let valid_signature = lock_algo.verify_signature(
            account_script.args().unpack(),
            withdrawal_request.signature(),
            message,
        )?;

        if !valid_signature {
            return Err(LockAlgorithmError::InvalidSignature.into());
        }

        // find user account
        let id = state
            .get_account_id_by_script_hash(&account_script_hash.into())?
            .ok_or(StateError::MissingKey)?; // find Simple UDT account

        // check balance
        let sudt_id = state
            .get_account_id_by_script_hash(&sudt_script_hash.into())?
            .ok_or(StateError::MissingKey)?;
        let balance = state.get_sudt_balance(sudt_id, id)?;
        if amount > balance {
            return Err(ValidateError::InvalidWithdrawal.into());
        }
        // check nonce
        let expected_nonce = state.get_nonce(id)?;
        let actual_nonce: u32 = raw.nonce().unpack();
        if actual_nonce != expected_nonce {
            return Err(ValidateError::InvalidWithdrawalNonce {
                expected: expected_nonce,
                actual: actual_nonce,
            }
            .into());
        }
        Ok(())
    }

    /// Apply l2 state transition
    ///
    /// Notice:
    /// This function do not verify the block and transactions signature.
    /// The caller is supposed to do the verification.
    pub fn apply_state_transition<S: State + CodeStore>(
        &self,
        state: &mut S,
        args: StateTransitionArgs,
    ) -> Result<(), Error> {
        let raw_block = args.l2block.raw();
        let withdrawal_requests: Vec<_> = args.l2block.withdrawal_requests().into_iter().collect();
        // apply withdrawal to state
        state.apply_withdrawal_requests(&withdrawal_requests)?;
        // apply deposition to state
        state.apply_deposition_requests(&args.deposition_requests)?;

        // handle transactions
        let block_info = get_block_info(&raw_block);
        let block_hash = raw_block.hash();
        for (tx_index, tx) in args.l2block.transactions().into_iter().enumerate() {
            let raw_tx = tx.raw();
            // build challenge context
            let challenge_context = StartChallenge::new_builder()
                .block_hash(block_hash.pack())
                .block_number(block_info.number())
                .tx_index((tx_index as u32).pack())
                .build();
            // check nonce
            let expected_nonce = state.get_nonce(raw_tx.from_id().unpack())?;
            let actual_nonce: u32 = raw_tx.nonce().unpack();
            if actual_nonce != expected_nonce {
                return Err(TransactionErrorWithContext::new(
                    challenge_context,
                    TransactionError::Nonce {
                        expected: expected_nonce,
                        actual: actual_nonce,
                    },
                )
                .into());
            }
            // build call context
            // NOTICE users only allowed to send HandleMessage CallType txs
            let run_result = match self.execute(state, &block_info, &raw_tx) {
                Ok(run_result) => run_result,
                Err(err) => {
                    return Err(TransactionErrorWithContext::new(challenge_context, err).into());
                }
            };
            state.apply_run_result(&run_result)?;
        }

        Ok(())
    }

    fn load_backend<S: State + CodeStore>(
        &self,
        state: &S,
        account_id: u32,
    ) -> Result<Option<Backend>, StateError> {
        let script_hash = state.get_script_hash(account_id)?;
        Ok(state
            .get_script(&script_hash)
            .and_then(|script| {
                // only accept data script hash type for now
                if script.hash_type() == ScriptHashType::Data.into() {
                    let code_hash: [u8; 32] = script.code_hash().unpack();
                    self.backend_manage.get_backend(&code_hash.into())
                } else {
                    None
                }
            })
            .cloned())
    }

    /// execute a layer2 tx
    pub fn execute<S: State + CodeStore>(
        &self,
        state: &S,
        block_info: &BlockInfo,
        raw_tx: &RawL2Transaction,
    ) -> Result<RunResult, TransactionError> {
        let mut run_result = RunResult::default();
        {
            let core_machine = Box::<AsmCoreMachine>::default();
            let machine_builder =
                DefaultMachineBuilder::new(core_machine).syscall(Box::new(L2Syscalls {
                    state,
                    block_info: block_info,
                    raw_tx,
                    result: &mut run_result,
                    code_store: state,
                }));
            let mut machine = AsmMachine::new(machine_builder.build(), None);
            let account_id = raw_tx.to_id().unpack();
            let backend = self
                .load_backend(state, account_id)?
                .ok_or(TransactionError::Backend { account_id })?;
            machine.load_program(&backend.generator, &[])?;
            let code = machine.run()?;
            if code != 0 {
                return Err(TransactionError::InvalidExitCode(code).into());
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
        Ok(run_result)
    }
}

fn get_block_info(l2block: &RawL2Block) -> BlockInfo {
    BlockInfo::new_builder()
        .aggregator_id(l2block.aggregator_id())
        .number(l2block.number())
        .timestamp(l2block.timestamp())
        .build()
}
