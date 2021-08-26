use crate::cancel_challenge::{build_output, CancelChallengeOutput};
use crate::enter_challenge::EnterChallenge;
use crate::offchain::{mock_poa::MockPoA, CKBGenesisInfo};
use crate::types::VerifyContext;
use crate::utils::transaction_skeleton::TransactionSkeleton;
use crate::utils::wallet::Wallet;

use anyhow::Result;
use gw_common::blake2b::new_blake2b;
use gw_common::H256;
use gw_config::BlockProducerConfig;
use gw_generator::ChallengeContext;
use gw_types::bytes::Bytes;
use gw_types::core::DepType;
use gw_types::offchain::{CellInfo, InputCellInfo, RollupContext};
use gw_types::packed::{
    Byte32, CellDep, CellInput, CellOutput, ChallengeTarget, ChallengeWitness, GlobalState,
    OutPoint, Script, ScriptOpt, Transaction, WitnessArgs,
};
use gw_types::prelude::{Builder, Entity, Pack, Unpack};

use std::collections::HashMap;

pub struct MockRollup {
    pub rollup_type_script: ScriptOpt,
    pub rollup_context: RollupContext,
    pub wallet: Wallet,
    pub config: BlockProducerConfig,
    pub ckb_genesis_info: CKBGenesisInfo,
    pub builtin_load_data: HashMap<H256, CellDep>,
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
    )?;

    let cancel_by_cell_deps =
        CancelByCellDeps::new(&mock_rollup.builtin_load_data, &mock_rollup.config);
    let verifier_context = cancel_by_cell_deps.mock_verifier(&mut cancel_output)?;

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
        mock_rollup.config.rollup_cell_type_dep.clone().into(),
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

    let challenge_dep = mock_rollup.config.challenge_cell_lock_dep.clone().into();
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
    if let Some(verifier_witness) = cancel_output.verifier_witness {
        tx_skeleton.witnesses_mut().push(verifier_witness);
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
    ) -> Self {
        MockRollup {
            rollup_type_script,
            rollup_context,
            wallet,
            config,
            ckb_genesis_info,
            builtin_load_data,
        }
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

struct CancelByCellDeps<'a> {
    builtin_load_data: &'a HashMap<H256, CellDep>,
    config: &'a BlockProducerConfig,
}

impl<'a> CancelByCellDeps<'a> {
    fn new(builtin_load_data: &'a HashMap<H256, CellDep>, config: &'a BlockProducerConfig) -> Self {
        CancelByCellDeps {
            builtin_load_data,
            config,
        }
    }

    fn mock_verifier(&self, cancel_output: &mut CancelChallengeOutput) -> Result<VerifierContext> {
        let load_data = {
            let load = cancel_output.load_data_cells.take();
            load.map(|ld| LoadData::new(ld, self.builtin_load_data))
        };
        let verifier_tx_hash = random_hash().unpack();
        let verifier_context = {
            let cell_dep = cancel_output.verifier_dep(self.config)?;
            let input = cancel_output.verifier_input(verifier_tx_hash, 0);
            let witness = cancel_output.verifier_witness.clone();
            let load_data_context = load_data.map(|ld| ld.into_context(verifier_tx_hash, 0));
            VerifierContext::new(cell_dep, input, witness, load_data_context)
        };

        Ok(verifier_context)
    }
}

fn random_hash() -> Byte32 {
    let mut hash = [0u8; 32];

    let mut hasher = new_blake2b();
    hasher.update(&rand::random::<u32>().to_le_bytes());
    hasher.finalize(&mut hash);
    hash.pack()
}

#[derive(Clone)]
struct LoadData {
    builtin: Vec<CellDep>,
    cells: Vec<(CellOutput, Bytes)>,
}

#[derive(Clone)]
struct LoadDataContext {
    builtin_cell_deps: Vec<CellDep>,
    cell_deps: Vec<CellDep>,
    inputs: Vec<InputCellInfo>,
}

impl LoadData {
    fn new(
        load_data_cells: HashMap<H256, (CellOutput, Bytes)>,
        builtin: &HashMap<H256, CellDep>,
    ) -> Self {
        let (load_builtin, load_data_cells): (HashMap<_, _>, HashMap<_, _>) = load_data_cells
            .into_iter()
            .partition(|(k, _v)| builtin.contains_key(k));

        let cells = load_data_cells.values().map(|v| (*v).to_owned()).collect();
        let builtin = {
            let to_dep = |k| -> CellDep { builtin.get(k).cloned().expect("should exists") };
            load_builtin.iter().map(|(k, _)| to_dep(k)).collect()
        };

        LoadData { builtin, cells }
    }

    fn into_context(self, verifier_tx_hash: H256, verifier_tx_index: u32) -> LoadDataContext {
        assert_eq!(verifier_tx_index, 0, "verifier cell should be first one");

        let to_context = |(idx, (output, data))| -> (CellDep, InputCellInfo) {
            let out_point = OutPoint::new_builder()
                .tx_hash(Into::<[u8; 32]>::into(verifier_tx_hash).pack())
                .index((idx as u32).pack())
                .build();

            let cell_dep = CellDep::new_builder()
                .out_point(out_point.clone())
                .dep_type(DepType::Code.into())
                .build();

            let input = CellInput::new_builder()
                .previous_output(out_point.clone())
                .build();

            let cell = CellInfo {
                out_point,
                output,
                data,
            };

            let cell_info = InputCellInfo { input, cell };

            (cell_dep, cell_info)
        };

        let (cell_deps, inputs) = {
            let cells = self.cells.into_iter().enumerate();
            let to_ctx = cells.map(|(idx, cell)| (idx + 1, cell)).map(to_context);
            to_ctx.unzip()
        };

        LoadDataContext {
            builtin_cell_deps: self.builtin,
            cell_deps,
            inputs,
        }
    }
}

#[derive(Clone)]
struct VerifierContext {
    cell_dep: CellDep,
    input: InputCellInfo,
    witness: Option<WitnessArgs>,
    load_data_context: Option<LoadDataContext>,
}

impl VerifierContext {
    fn new(
        cell_dep: CellDep,
        input: InputCellInfo,
        witness: Option<WitnessArgs>,
        load_data_context: Option<LoadDataContext>,
    ) -> Self {
        VerifierContext {
            cell_dep,
            input,
            witness,
            load_data_context,
        }
    }
}
