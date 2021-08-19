use crate::types::{VerifyContext, VerifyWitness};

use anyhow::{anyhow, Result};
use ckb_types::prelude::{Builder, Entity};
use gw_common::H256;
use gw_config::BlockProducerConfig;
use gw_types::core::Status;
use gw_types::offchain::{CellInfo, InputCellInfo, RollupContext};
use gw_types::packed::{
    CellDep, CellInput, CellOutput, GlobalState, OutPoint, RollupAction, RollupActionUnion,
    RollupCancelChallenge, Script, VerifyTransactionSignatureWitness, VerifyTransactionWitness,
    VerifyWithdrawalWitness, WitnessArgs,
};
use gw_types::prelude::Unpack;
use gw_types::{bytes::Bytes, prelude::Pack as GWPack};
use std::collections::HashMap;

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

pub struct CancelChallengeOutput {
    pub post_global_state: GlobalState,
    pub verifier_cell: (CellOutput, Bytes),
    pub load_data_cells: Option<HashMap<H256, (CellOutput, Bytes)>>, // Some for transaction execution verifiction, sys_load_data
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

    pub fn verifier_dep(&self, block_producer_config: &BlockProducerConfig) -> Result<CellDep> {
        let lock_code_hash: [u8; 32] = self.verifier_cell.0.lock().code_hash().unpack();
        let mut allowed_script_deps = {
            let eoa = block_producer_config.allowed_eoa_deps.iter();
            eoa.chain(block_producer_config.allowed_contract_deps.iter())
        };
        let has_dep = allowed_script_deps.find(|(code_hash, _)| code_hash.0 == lock_code_hash);
        let to_dep = has_dep.map(|(_, dep)| dep.clone().into());
        to_dep.ok_or_else(|| anyhow!("verifier lock dep not found"))
    }
}

pub fn build_output(
    rollup_context: &RollupContext,
    prev_global_state: GlobalState,
    challenge_cell: &CellInfo,
    burn_lock: Script,
    owner_lock: Script,
    context: VerifyContext,
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
            Ok(cancel.build_output(data, Some(verifier_witness), None))
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
            Ok(cancel.build_output(data, Some(verifier_witness), None))
        }
        VerifyWitness::TxExecution { witness, load_data } => {
            let verifier_lock = context
                .receiver_script
                .ok_or_else(|| anyhow!("receiver script not found"))?;

            let cancel: CancelChallenge<VerifyTransactionWitness> = CancelChallenge::new(
                prev_global_state,
                rollup_context,
                challenge_cell,
                burn_lock,
                owner_lock,
                verifier_lock,
                witness,
            );

            let data = cancel.build_verifier_data();
            let load_data = load_data.into_iter().map(|(k, v)| (k, v.unpack()));
            Ok(cancel.build_output(data, None, Some(load_data.collect())))
        }
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
        load_data: Option<HashMap<H256, Bytes>>,
    ) -> CancelChallengeOutput {
        let build_cell = |data: Bytes, lock: Script| -> (CellOutput, Bytes) {
            let dummy_output = CellOutput::new_builder()
                .capacity(100_000_000u64.pack())
                .lock(lock)
                .build();

            let capacity = dummy_output
                .occupied_capacity(data.len())
                .expect("impossible cancel challenge verify cell overflow");

            let output = dummy_output.as_builder().capacity(capacity.pack()).build();

            (output, data)
        };

        let verifier_cell = build_cell(verifier_data, self.verifier_lock);

        let owner_lock = self.owner_lock;
        let load_data_cells = load_data.map(|data| {
            data.into_iter()
                .map(|(k, v)| (k, build_cell(v, owner_lock.clone())))
                .collect()
        });

        let burn = Burn::new(self.challenge_cell, self.reward_burn_rate);
        let burn_output = burn.build_output(self.burn_lock);

        let post_global_state = build_post_global_state(self.prev_global_state);
        let challenge_witness = WitnessArgs::new_builder()
            .lock(Some(self.verify_witness.as_bytes()).pack())
            .build();

        CancelChallengeOutput {
            post_global_state,
            verifier_cell,
            load_data_cells,
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
