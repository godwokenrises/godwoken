use crate::cancel_challenge::{
    build_output, CancelChallengeOutput, LoadData, LoadDataContext, LoadDataStrategy,
    RecoverAccounts, RecoverAccountsContext,
};
use crate::enter_challenge::EnterChallenge;
use crate::offchain::{mock_poa::MockPoA, CKBGenesisInfo};
use crate::types::VerifyContext;
use gw_rpc_client::contract::ContractsCellDepManager;
use gw_utils::transaction_skeleton::TransactionSkeleton;
use gw_utils::wallet::Wallet;

use anyhow::Result;
use arc_swap::Guard;
use gw_common::blake2b::new_blake2b;
use gw_common::H256;
use gw_config::{BlockProducerConfig, ContractsCellDep};
use gw_generator::ChallengeContext;
use gw_types::bytes::Bytes;
use gw_types::offchain::{CellInfo, InputCellInfo, RollupContext};
use gw_types::packed::{
    Byte32, CellDep, CellInput, CellOutput, ChallengeTarget, ChallengeWitness, GlobalState,
    OutPoint, Script, ScriptOpt, Transaction, WitnessArgs,
};
use gw_types::prelude::{Builder, Entity, Pack, Unpack};

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

pub struct MockRollup {
    pub rollup_type_script: ScriptOpt,
    pub rollup_context: RollupContext,
    pub wallet: Wallet,
    pub config: BlockProducerConfig,
    pub ckb_genesis_info: CKBGenesisInfo,
    pub builtin_load_data: HashMap<H256, CellDep>,
    pub contracts_dep_manager: ContractsCellDepManager,
}

#[derive(Clone)]
pub struct MockOutput {
    pub cell_deps: Vec<InputCellInfo>,
    pub inputs: Vec<InputCellInfo>,

    pub tx: Transaction,
}

pub fn mock_cancel_challenge_tx(
    mock_rollup: &MockRollup,
    mock_poa: &MockPoA,
    global_state: GlobalState,
    challenge_target: ChallengeTarget,
    context: VerifyContext,
    load_data_strategy: Option<LoadDataStrategy>,
) -> Result<MockOutput> {
    let burn_lock = {
        let challenger_config = &mock_rollup.config.challenger_config;
        challenger_config.burn_lock.clone().into()
    };
    let owner_lock = mock_rollup.wallet.lock_script().to_owned();

    let challenge_input = mock_rollup.mock_challenge_cell(challenge_target);
    let mut cancel_output = build_output(
        &mock_rollup.rollup_context,
        global_state.clone(),
        &challenge_input.cell,
        burn_lock,
        owner_lock,
        context,
        &mock_rollup.builtin_load_data,
        load_data_strategy,
    )?;

    let contracts_dep = mock_rollup.cell_deps();
    let verifier_context = VerifierContext::mock_from(&mut cancel_output, &contracts_dep)?;
    let contracts_dep = mock_rollup.contracts_dep_manager.load();

    let mut tx_skeleton = TransactionSkeleton::default();
    let mut cell_deps = Vec::new();
    let mut inputs = Vec::new();

    // Rollup
    let mut rollup_input = mock_rollup.mock_rollup_cell(global_state, mock_poa.lock.clone());
    rollup_input.input = {
        let builder = rollup_input.input.as_builder();
        builder.since(mock_poa.input_since.pack()).build()
    };
    inputs.push(rollup_input.clone());

    let rollup_deps = vec![
        contracts_dep.rollup_cell_type.clone().into(),
        mock_rollup.config.rollup_config_cell_dep.clone().into(),
    ];
    let rollup_output = (
        rollup_input.cell.output.clone(),
        cancel_output.post_global_state.as_bytes(),
    );
    let rollup_witness = cancel_output.rollup_witness;

    tx_skeleton.cell_deps_mut().extend(rollup_deps);
    tx_skeleton.inputs_mut().push(rollup_input);
    tx_skeleton.outputs_mut().push(rollup_output);
    tx_skeleton.witnesses_mut().push(rollup_witness);

    // Challenge
    inputs.push(challenge_input.clone());

    let challenge_dep = contracts_dep.challenge_cell_lock.clone().into();
    let challenge_witness = cancel_output.challenge_witness;
    tx_skeleton.cell_deps_mut().push(challenge_dep);
    tx_skeleton.inputs_mut().push(challenge_input);
    tx_skeleton.witnesses_mut().push(challenge_witness);

    // Verifier
    inputs.push(verifier_context.input.clone());

    tx_skeleton.cell_deps_mut().push(verifier_context.cell_dep);
    if let Some(load_data_context) = verifier_context.load_data_context {
        let load_builtin_cell_deps = load_data_context.builtin_cell_deps;
        let load_cell_deps = load_data_context.cell_deps;
        tx_skeleton.cell_deps_mut().extend(load_builtin_cell_deps);
        tx_skeleton.cell_deps_mut().extend(load_cell_deps);

        cell_deps.extend(load_data_context.inputs);
    }
    tx_skeleton.inputs_mut().push(verifier_context.input);
    if let Some(verifier_witness) = verifier_context.witness {
        tx_skeleton.witnesses_mut().push(verifier_witness);
    }

    // Recover Accounts
    if let Some(recover_accounts_context) = verifier_context.recover_accounts_context {
        inputs.extend(recover_accounts_context.inputs.clone());

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
    let burn_cells = cancel_output.burn_cells;
    tx_skeleton.outputs_mut().extend(burn_cells);

    // Signature verification needs an owner cell
    let owner_cell = mock_rollup.mock_owner_cell();
    inputs.push(owner_cell.clone());

    // Poa
    inputs.extend(mock_poa.inputs.clone());

    let poa_cell_deps = mock_poa.cell_deps.clone();
    tx_skeleton.cell_deps_mut().extend(poa_cell_deps);
    tx_skeleton.inputs_mut().extend(mock_poa.inputs.clone());
    tx_skeleton.outputs_mut().extend(mock_poa.outputs.clone());

    let owner_dep = mock_rollup.ckb_genesis_info.sighash_dep.clone();
    tx_skeleton.cell_deps_mut().push(owner_dep);
    tx_skeleton.inputs_mut().push(owner_cell);

    // ensure no cell dep duplicate
    {
        let deps: HashSet<_> = tx_skeleton.cell_deps_mut().iter().collect();
        *tx_skeleton.cell_deps_mut() = deps.into_iter().cloned().collect();
    }

    let owner_lock = mock_rollup.wallet.lock_script().to_owned();
    mock_rollup.fill_tx_fee(&mut tx_skeleton, owner_lock)?;
    let tx = mock_rollup.wallet.sign_tx_skeleton(tx_skeleton)?;

    Ok(MockOutput {
        cell_deps,
        inputs,
        tx,
    })
}

impl MockRollup {
    pub fn new(
        rollup_type_script: ScriptOpt,
        rollup_context: RollupContext,
        wallet: Wallet,
        ckb_genesis_info: CKBGenesisInfo,
        config: BlockProducerConfig,
        builtin_load_data: HashMap<H256, CellDep>,
        contracts_dep_manager: ContractsCellDepManager,
    ) -> Self {
        MockRollup {
            rollup_type_script,
            rollup_context,
            wallet,
            config,
            ckb_genesis_info,
            builtin_load_data,
            contracts_dep_manager,
        }
    }

    pub fn cell_deps(&self) -> Guard<Arc<ContractsCellDep>> {
        self.contracts_dep_manager.load()
    }

    fn mock_owner_cell(&self) -> InputCellInfo {
        let out_point = OutPoint::new_builder()
            .tx_hash(random_hash())
            .index(0u32.pack())
            .build();

        let input = CellInput::new_builder()
            .previous_output(out_point.clone())
            .build();

        let output = CellOutput::new_builder()
            .capacity((u64::max_value() / 2).pack())
            .lock(self.wallet.lock_script().to_owned())
            .build();

        let cell = CellInfo {
            out_point,
            output,
            data: Bytes::new(),
        };

        InputCellInfo { input, cell }
    }

    fn mock_rollup_cell(&self, global_state: GlobalState, lock: Script) -> InputCellInfo {
        let out_point = OutPoint::new_builder()
            .tx_hash(random_hash())
            .index(0u32.pack())
            .build();

        let input = CellInput::new_builder()
            .previous_output(out_point.clone())
            .build();

        let output = {
            let rollup_output = CellOutput::new_builder()
                .capacity(100_000_000u64.pack())
                .type_(self.rollup_type_script.clone())
                .lock(lock)
                .build();

            let capacity = rollup_output
                .occupied_capacity(global_state.as_bytes().len())
                .expect("rollup capacity overflow");

            rollup_output.as_builder().capacity(capacity.pack()).build()
        };

        let cell = CellInfo {
            out_point,
            output,
            data: global_state.as_bytes(),
        };

        InputCellInfo { input, cell }
    }

    fn mock_challenge_cell(&self, target: ChallengeTarget) -> InputCellInfo {
        let challenge_context = ChallengeContext {
            target,
            witness: ChallengeWitness::default(),
        };
        let rewards_lock = {
            let challenger_config = &self.config.challenger_config;
            challenger_config.rewards_receiver_lock.clone().into()
        };

        let enter_challenge = EnterChallenge::new(
            GlobalState::default(),
            &self.rollup_context,
            challenge_context,
            rewards_lock,
        );
        let challenge_output = enter_challenge.build_output();

        let out_point = OutPoint::new_builder()
            .tx_hash(random_hash())
            .index(0u32.pack())
            .build();

        let input = CellInput::new_builder()
            .previous_output(out_point.clone())
            .build();

        let (output, data) = challenge_output.challenge_cell;

        let cell = CellInfo {
            out_point,
            output,
            data,
        };

        InputCellInfo { input, cell }
    }

    fn fill_tx_fee(
        &self,
        tx_skeleton: &mut TransactionSkeleton,
        lock_script: Script,
    ) -> Result<()> {
        const CHANGE_CELL_CAPACITY: u64 = 61_00000000;

        let estimate_tx_size_with_change =
            |tx_skeleton: &mut TransactionSkeleton| -> Result<usize> {
                let change_cell = CellOutput::new_builder()
                    .lock(lock_script.clone())
                    .capacity(CHANGE_CELL_CAPACITY.pack())
                    .build();

                tx_skeleton
                    .outputs_mut()
                    .push((change_cell, Default::default()));

                let tx_size = tx_skeleton.tx_in_block_size()?;
                tx_skeleton.outputs_mut().pop();

                Ok(tx_size)
            };

        // calculate required fee
        let tx_size = estimate_tx_size_with_change(tx_skeleton)?;
        let tx_fee = tx_size as u64;
        let max_paid_fee = tx_skeleton
            .calculate_fee()?
            .saturating_sub(CHANGE_CELL_CAPACITY);

        let mut required_fee = tx_fee.saturating_sub(max_paid_fee);
        if 0 == required_fee {
            let change_capacity = max_paid_fee + CHANGE_CELL_CAPACITY - tx_fee;
            let change_cell = CellOutput::new_builder()
                .lock(lock_script.clone())
                .capacity(change_capacity.pack())
                .build();

            tx_skeleton
                .outputs_mut()
                .push((change_cell, Default::default()));

            return Ok(());
        }

        required_fee += CHANGE_CELL_CAPACITY;

        let mut change_capacity = 0;
        while required_fee > 0 {
            // to filter used input cells
            tx_skeleton.inputs_mut().push(self.mock_owner_cell());

            let tx_size = estimate_tx_size_with_change(tx_skeleton)?;
            let tx_fee = tx_size as u64;
            let max_paid_fee = tx_skeleton
                .calculate_fee()?
                .saturating_sub(CHANGE_CELL_CAPACITY);

            required_fee = tx_fee.saturating_sub(max_paid_fee);
            change_capacity = max_paid_fee + CHANGE_CELL_CAPACITY - tx_fee;
        }

        let change_cell = CellOutput::new_builder()
            .lock(lock_script)
            .capacity(change_capacity.pack())
            .build();

        tx_skeleton
            .outputs_mut()
            .push((change_cell, Default::default()));

        Ok(())
    }
}

fn random_hash() -> Byte32 {
    let mut hash = [0u8; 32];

    let mut hasher = new_blake2b();
    hasher.update(&rand::random::<u32>().to_le_bytes());
    hasher.finalize(&mut hash);
    hash.pack()
}

struct VerifierContext {
    cell_dep: CellDep,
    input: InputCellInfo,
    witness: Option<WitnessArgs>,
    load_data_context: Option<LoadDataContext>,
    recover_accounts_context: Option<RecoverAccountsContext>,
}

impl VerifierContext {
    fn mock_from(
        cancel_output: &mut CancelChallengeOutput,
        contracts_dep: &ContractsCellDep,
    ) -> Result<VerifierContext> {
        let verifier_tx_hash = random_hash().unpack();
        let load_data_len = {
            let load_data = cancel_output.load_data.as_ref();
            load_data.map(|l| l.cell_len()).unwrap_or(0)
        };

        let to_ctx = |load_data: LoadData| load_data.into_context(verifier_tx_hash, 0);
        let to_recv_ctx = |recover_accounts: RecoverAccounts| -> Result<_> {
            recover_accounts.into_context(verifier_tx_hash, load_data_len + 1, contracts_dep)
        };

        let verifier_context = VerifierContext {
            cell_dep: cancel_output.verifier_dep(contracts_dep)?,
            input: cancel_output.verifier_input(verifier_tx_hash, 0),
            witness: cancel_output.verifier_witness.clone(),
            load_data_context: cancel_output.load_data.take().map(to_ctx),
            recover_accounts_context: {
                let accounts = cancel_output.recover_accounts.take();
                accounts.map(to_recv_ctx).transpose()?
            },
        };

        Ok(verifier_context)
    }
}
