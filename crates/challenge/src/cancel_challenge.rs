use crate::types::{VerifyContext, VerifyWitness};

use anyhow::{anyhow, Result};
use ckb_types::prelude::{Builder, Entity};
use gw_common::H256;
use gw_config::ContractsCellDep;
use gw_types::core::{DepType, SigningType, Status};
use gw_types::offchain::{CellInfo, InputCellInfo, RecoverAccount, RollupContext};
use gw_types::packed::{
    CellDep, CellInput, CellOutput, GlobalState, OutPoint, RollupAction, RollupActionUnion,
    RollupCancelChallenge, Script, VerifyTransactionSignatureWitness, VerifyTransactionWitness,
    VerifyWithdrawalWitness, WitnessArgs,
};
use gw_types::prelude::Unpack;
use gw_types::{bytes::Bytes, prelude::Pack as GWPack};
use std::collections::{HashMap, HashSet};

pub struct CancelChallenge<'a, W: Entity> {
    rollup_type_hash: H256,
    reward_burn_rate: u8,
    prev_global_state: GlobalState,
    challenge_cell: &'a CellInfo,
    verifier_lock: Script,
    burn_lock: Script,
    owner_lock: Script,
    verify_witness: W,
}

#[derive(Clone)]
pub struct LoadData {
    pub builtin: Vec<CellDep>,
    pub cells: Vec<(CellOutput, Bytes)>,
}

#[derive(Clone)]
pub struct LoadDataContext {
    pub builtin_cell_deps: Vec<CellDep>,
    pub cell_deps: Vec<CellDep>,
    pub inputs: Vec<InputCellInfo>,
}

#[derive(Clone)]
pub struct RecoverAccounts {
    pub cells: Vec<(CellOutput, Bytes)>,
    pub witnesses: Vec<WitnessArgs>,
}

#[derive(Clone)]
pub struct RecoverAccountsContext {
    pub cell_deps: Vec<CellDep>,
    pub inputs: Vec<InputCellInfo>,
    pub witnesses: Vec<WitnessArgs>,
}

pub struct CancelChallengeOutput {
    pub post_global_state: GlobalState,
    pub load_data: Option<LoadData>, // Some for transaction execution verification, sys_load_data
    pub recover_accounts: Option<RecoverAccounts>,
    pub verifier_cell: (CellOutput, Bytes),
    pub burn_cells: Vec<(CellOutput, Bytes)>,
    pub verifier_witness: Option<WitnessArgs>, // Some for signature verification
    pub challenge_witness: WitnessArgs,
    pub rollup_witness: WitnessArgs,
}

impl CancelChallengeOutput {
    pub fn verifier_input(&self, tx_hash: H256, tx_index: u32) -> InputCellInfo {
        let (output, data) = self.verifier_cell.clone();
        let tx_hash: [u8; 32] = tx_hash.into();

        let out_point = OutPoint::new_builder()
            .tx_hash(tx_hash.pack())
            .index(tx_index.pack())
            .build();

        let input = CellInput::new_builder()
            .previous_output(out_point.clone())
            .build();

        let cell = CellInfo {
            out_point,
            output,
            data,
        };

        InputCellInfo { input, cell }
    }

    pub fn verifier_dep(&self, contracts_dep: &ContractsCellDep) -> Result<CellDep> {
        let lock_code_hash: [u8; 32] = self.verifier_cell.0.lock().code_hash().unpack();
        let mut allowed_script_deps = {
            let eoa = contracts_dep.allowed_eoa_locks.iter();
            eoa.chain(contracts_dep.allowed_contract_types.iter())
        };
        let has_dep = allowed_script_deps.find(|(code_hash, _)| code_hash.0 == lock_code_hash);
        let to_dep = has_dep.map(|(_, dep)| dep.clone().into());
        if to_dep.is_none() {
            let lock_code_hash = ckb_types::H256::from(lock_code_hash);
            log::error!("lock code {} not found", lock_code_hash);
        }
        to_dep.ok_or_else(|| anyhow!("verifier lock dep not found"))
    }
}

#[derive(Debug, Clone, Copy)]
pub enum LoadDataStrategy {
    Witness,
    CellDep,
}

impl Default for LoadDataStrategy {
    fn default() -> Self {
        LoadDataStrategy::Witness
    }
}

#[allow(clippy::too_many_arguments)]
pub fn build_output(
    rollup_context: &RollupContext,
    prev_global_state: GlobalState,
    challenge_cell: &CellInfo,
    burn_lock: Script,
    owner_lock: Script,
    context: VerifyContext,
    builtin_load_data: &HashMap<H256, CellDep>,
    load_data_strategy: Option<LoadDataStrategy>,
) -> Result<CancelChallengeOutput> {
    match context.verify_witness {
        VerifyWitness::Withdrawal(witness) => {
            let verifier_lock = context.sender_script;

            let verifier_witness = {
                let signature = witness.withdrawal_request().signature();
                WitnessArgs::new_builder()
                    .lock(Some(signature).pack())
                    .build()
            };

            let cancel: CancelChallenge<VerifyWithdrawalWitness> = CancelChallenge::new(
                prev_global_state,
                rollup_context,
                challenge_cell,
                burn_lock,
                owner_lock,
                verifier_lock,
                witness,
            );

            let data = cancel.build_verifier_data();
            Ok(cancel.build_output(data, Some(verifier_witness), None, None))
        }
        VerifyWitness::TxSignature(witness) => {
            let verifier_lock = context.sender_script;
            let receiver_script = context
                .receiver_script
                .ok_or_else(|| anyhow!("receiver script not found"))?;

            let verifier_witness = {
                let signature = witness.l2tx().signature();
                WitnessArgs::new_builder()
                    .lock(Some(signature).pack())
                    .build()
            };

            let cancel: CancelChallenge<VerifyTransactionSignatureWitness> = CancelChallenge::new(
                prev_global_state,
                rollup_context,
                challenge_cell,
                burn_lock,
                owner_lock,
                verifier_lock,
                witness,
            );

            let data = cancel.build_verifier_data(receiver_script.hash().into());
            Ok(cancel.build_output(data, Some(verifier_witness), None, None))
        }
        VerifyWitness::TxExecution {
            witness,
            load_data,
            recover_accounts,
        } => {
            let verifier_lock = context
                .receiver_script
                .ok_or_else(|| anyhow!("receiver script not found"))?;

            let (load_builtin, load_data): (HashMap<_, _>, HashMap<_, _>) = load_data
                .into_iter()
                .partition(|(k, _v)| builtin_load_data.contains_key(k));

            let builtin_deps = {
                let to_dep = |(k, _)| -> CellDep {
                    builtin_load_data.get(k).cloned().expect("should exists")
                };
                load_builtin.iter().map(to_dep).collect()
            };

            let load_data: Vec<Bytes> = load_data.into_iter().map(|(_, v)| v.unpack()).collect();
            let (witness, load_data_cells) = match load_data_strategy.unwrap_or_default() {
                LoadDataStrategy::Witness => {
                    let context = {
                        let builder = witness.context().as_builder();
                        builder.load_data(load_data.pack()).build()
                    };
                    let witness = witness.as_builder().context(context).build();
                    (witness, vec![])
                }
                LoadDataStrategy::CellDep => {
                    let to_cell = |v| build_cell(v, owner_lock.clone());
                    let cells = load_data.into_iter().map(to_cell).collect();
                    (witness, cells)
                }
            };

            let cancel: CancelChallenge<VerifyTransactionWitness> = CancelChallenge::new(
                prev_global_state,
                rollup_context,
                challenge_cell,
                burn_lock,
                owner_lock.clone(),
                verifier_lock,
                witness,
            );

            let data = cancel.build_verifier_data();
            let load_data = LoadData {
                builtin: builtin_deps,
                cells: load_data_cells,
            };

            let recover_accounts = {
                let owner_lock_hash = owner_lock.hash().into();
                let accounts = recover_accounts.into_iter();
                let to_cell = accounts.map(|a| build_recover_account_cell(owner_lock_hash, a));
                let (cells, witnesses) = to_cell.unzip();
                RecoverAccounts { cells, witnesses }
            };

            Ok(cancel.build_output(data, None, Some(load_data), Some(recover_accounts)))
        }
    }
}

impl LoadData {
    pub fn cell_len(&self) -> usize {
        self.cells.len()
    }

    pub fn into_context(self, verifier_tx_hash: H256, verifier_tx_index: u32) -> LoadDataContext {
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

impl RecoverAccounts {
    pub fn into_context(
        self,
        verifier_tx_hash: H256,
        index_offset: usize,
        contracts_dep: &ContractsCellDep,
    ) -> Result<RecoverAccountsContext> {
        assert!(index_offset != 0, "verifier cell should be first one");
        let RecoverAccounts { cells, witnesses } = self;

        let cell_deps = {
            let allowed_eoa_deps = &contracts_dep.allowed_eoa_locks;
            let accounts = cells.iter();
            let to_code_hash: HashSet<_> = accounts
                .map(|(output, _)| output.lock().code_hash().unpack())
                .collect();

            let to_dep = to_code_hash.into_iter().map(|hash| {
                let maybe_dep = allowed_eoa_deps.get(&hash).cloned();
                let to_packed = maybe_dep.map(|d| d.into());
                to_packed.ok_or_else(|| anyhow!("recover account lock {} dep not found", hash))
            });

            to_dep.collect::<Result<Vec<CellDep>>>()?
        };

        let to_context = |(idx, (output, data))| -> InputCellInfo {
            let out_point = OutPoint::new_builder()
                .tx_hash(Into::<[u8; 32]>::into(verifier_tx_hash).pack())
                .index((idx as u32).pack())
                .build();

            let input = CellInput::new_builder()
                .previous_output(out_point.clone())
                .build();

            let cell = CellInfo {
                out_point,
                output,
                data,
            };

            InputCellInfo { input, cell }
        };

        let inputs = {
            let accounts = cells.into_iter().enumerate();
            let add_offset = accounts.map(|(idx, cell)| (idx + index_offset, cell));
            add_offset.map(to_context).collect()
        };

        Ok(RecoverAccountsContext {
            cell_deps,
            inputs,
            witnesses,
        })
    }
}

impl<'a, W: Entity> CancelChallenge<'a, W> {
    pub fn new(
        prev_global_state: GlobalState,
        rollup_context: &RollupContext,
        challenge_cell: &'a CellInfo,
        burn_lock: Script,
        owner_lock: Script,
        verifier_lock: Script,
        verify_witness: W,
    ) -> Self {
        let rollup_type_hash = rollup_context.rollup_script_hash;
        let reward_burn_rate = rollup_context.rollup_config.reward_burn_rate().into();

        Self {
            rollup_type_hash,
            reward_burn_rate,
            prev_global_state,
            challenge_cell,
            burn_lock,
            owner_lock,
            verifier_lock,
            verify_witness,
        }
    }

    pub fn build_output(
        self,
        verifier_data: Bytes,
        verifier_witness: Option<WitnessArgs>,
        load_data: Option<LoadData>,
        recover_accounts: Option<RecoverAccounts>,
    ) -> CancelChallengeOutput {
        let verifier_cell = build_cell(verifier_data, self.verifier_lock);

        let burn = Burn::new(self.challenge_cell, self.reward_burn_rate);
        let burn_output = burn.build_output(self.burn_lock);

        let post_global_state = build_post_global_state(self.prev_global_state);
        let challenge_witness = WitnessArgs::new_builder()
            .lock(Some(self.verify_witness.as_bytes()).pack())
            .build();

        CancelChallengeOutput {
            post_global_state,
            verifier_cell,
            load_data,
            recover_accounts,
            burn_cells: burn_output.burn_cells,
            verifier_witness,
            challenge_witness,
            rollup_witness: build_rollup_witness(),
        }
    }
}

impl<'a> CancelChallenge<'a, VerifyTransactionWitness> {
    fn build_verifier_data(&self) -> Bytes {
        self.owner_lock.hash().to_vec().into()
    }
}

impl<'a> CancelChallenge<'a, VerifyTransactionSignatureWitness> {
    // owner_lock_hash(32 bytes) | message(32 bytes)
    pub fn build_verifier_data(&self, receiver_script_hash: H256) -> Bytes {
        let owner_lock_hash = self.owner_lock.hash();
        let message = self.calc_tx_message(&receiver_script_hash);

        let mut data = [0u8; 64];
        data[0..32].copy_from_slice(&owner_lock_hash);
        data[32..64].copy_from_slice(&message);

        data.to_vec().into()
    }

    fn calc_tx_message(&self, receiver_script_hash: &H256) -> [u8; 32] {
        let raw_tx = self.verify_witness.l2tx().raw();
        raw_tx
            .calc_message(
                &self.rollup_type_hash,
                &H256::from(self.verifier_lock.hash()),
                receiver_script_hash,
            )
            .into()
    }
}

impl<'a> CancelChallenge<'a, VerifyWithdrawalWitness> {
    // owner_lock_hash(32 bytes) | message(32 bytes)
    pub fn build_verifier_data(&self) -> Bytes {
        let owner_lock_hash = self.owner_lock.hash();
        let message = self.calc_withdrawal_message();

        let mut data = [0u8; 64];
        data[0..32].copy_from_slice(&owner_lock_hash);
        data[32..64].copy_from_slice(&message);

        data.to_vec().into()
    }

    fn calc_withdrawal_message(&self) -> [u8; 32] {
        let raw_withdrawal = self.verify_witness.withdrawal_request().raw();
        raw_withdrawal.calc_message(&self.rollup_type_hash).into()
    }
}

struct Burn {
    burn_capacity: u128,
}

struct BurnOutput {
    burn_cells: Vec<(CellOutput, Bytes)>,
}

impl Burn {
    fn new(challenge_cell: &CellInfo, reward_burn_rate: u8) -> Self {
        let to_capacity = |c: &CellInfo| c.output.capacity().unpack() as u128;
        let challenge_capacity = to_capacity(challenge_cell);

        let burn_capacity = challenge_capacity.saturating_mul(reward_burn_rate.into()) / 100;

        Self { burn_capacity }
    }

    fn build_output(self, burn_lock: Script) -> BurnOutput {
        let build_outputs = |total_capacity: u128, lock: Script| -> Vec<(CellOutput, Bytes)> {
            let build = |capacity: u64, lock: Script| -> (CellOutput, Bytes) {
                let output = CellOutput::new_builder()
                    .capacity(capacity.pack())
                    .lock(lock)
                    .build();
                (output, Bytes::new())
            };

            let mut outputs = Vec::new();
            if total_capacity < u64::MAX as u128 {
                outputs.push(build(total_capacity as u64, lock));
                return outputs;
            }

            let min_capacity = (8 + lock.as_slice().len()) as u64 * 100_000_000;
            let mut remaind = total_capacity;
            while remaind > 0 {
                let max = remaind.saturating_sub(min_capacity as u128);
                match max.checked_sub(u64::MAX as u128) {
                    Some(cap) => {
                        outputs.push(build(u64::MAX, lock.clone()));
                        remaind = cap.saturating_add(min_capacity as u128);
                    }
                    None if max.saturating_add(min_capacity as u128) > u64::MAX as u128 => {
                        let max = max.saturating_add(min_capacity as u128);
                        let half = max / 2;
                        outputs.push(build(half as u64, lock.clone()));
                        outputs.push(build(max.saturating_sub(half) as u64, lock.clone()));
                        remaind = 0;
                    }
                    None => {
                        let cap = (max as u64).saturating_add(min_capacity);
                        outputs.push(build(cap, lock.clone()));
                        remaind = 0;
                    }
                }
            }
            outputs
        };

        BurnOutput {
            burn_cells: build_outputs(self.burn_capacity, burn_lock),
        }
    }
}

fn build_post_global_state(prev_global_state: GlobalState) -> GlobalState {
    let running_status: u8 = Status::Running.into();

    prev_global_state
        .as_builder()
        .status(running_status.into())
        .build()
}

fn build_rollup_witness() -> WitnessArgs {
    let cancel_challenge = RollupCancelChallenge::new_builder().build();

    let rollup_action = RollupAction::new_builder()
        .set(RollupActionUnion::RollupCancelChallenge(cancel_challenge))
        .build();

    WitnessArgs::new_builder()
        .output_type(Some(rollup_action.as_bytes()).pack())
        .build()
}

fn build_cell(data: Bytes, lock: Script) -> (CellOutput, Bytes) {
    let dummy_output = CellOutput::new_builder()
        .capacity(100_000_000u64.pack())
        .lock(lock)
        .build();

    let capacity = dummy_output
        .occupied_capacity(data.len())
        .expect("impossible cancel challenge verify cell overflow");

    let output = dummy_output.as_builder().capacity(capacity.pack()).build();

    (output, data)
}

fn build_recover_account_cell(
    owner_lock_hash: H256,
    account: RecoverAccount,
) -> ((CellOutput, Bytes), WitnessArgs) {
    let mut data = [0u8; 65];
    data[0..32].copy_from_slice(&owner_lock_hash.as_slice()[..32]);
    data[32] = SigningType::Raw.into();
    data[33..65].copy_from_slice(&account.message.as_slice()[..32]);

    let (output, data) = build_cell(data.to_vec().into(), account.lock_script);
    let witness = WitnessArgs::new_builder()
        .lock(Some(Into::<Bytes>::into(account.signature)).pack())
        .build();

    ((output, data), witness)
}
