use crate::types::{CellInfo, InputCellInfo};

use anyhow::{anyhow, Result};
use ckb_types::prelude::{Builder, Entity};
use gw_chain::challenge::{VerifyContext, VerifyWitness};
use gw_common::blake2b::new_blake2b;
use gw_common::H256;
use gw_config::BlockProducerConfig;
use gw_generator::RollupContext;
use gw_types::core::Status;
use gw_types::packed::{
    CellDep, CellInput, CellOutput, GlobalState, OutPoint, RollupAction, RollupActionUnion,
    RollupCancelChallenge, Script, VerifyTransactionSignatureWitness, VerifyTransactionWitness,
    VerifyWithdrawalWitness, WitnessArgs,
};
use gw_types::prelude::Unpack;
use gw_types::{bytes::Bytes, prelude::Pack as GWPack};

pub struct CancelChallenge<W: Entity> {
    rollup_type_hash: H256,
    prev_global_state: GlobalState,
    verifier_lock: Script,
    owner_lock: Script,
    verify_witness: W,
}

pub struct CancelChallengeOutput {
    pub post_global_state: GlobalState,
    pub verifier_cell: (CellOutput, Bytes),
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
                owner_lock,
                verifier_lock,
                witness,
            );

            let data = cancel.build_verifier_data();
            Ok(cancel.build_output(data, Some(verifier_witness)))
        }
        VerifyWitness::TxSignature(witness) => {
            let verifier_lock = context.sender_script;
            let receiver_script = context
                .receiver_script
                .ok_or_else(|| anyhow!("receiver script not found"))?;

            let verifier_witness = {
                let signature = witness.l2tx().signature().clone();
                WitnessArgs::new_builder()
                    .lock(Some(signature).pack())
                    .build()
            };

            let cancel: CancelChallenge<VerifyTransactionSignatureWitness> = CancelChallenge::new(
                prev_global_state,
                rollup_context,
                owner_lock,
                verifier_lock,
                witness,
            );

            let data = cancel.build_verifier_data(receiver_script.hash().into());
            Ok(cancel.build_output(data, Some(verifier_witness)))
        }
        VerifyWitness::TxExecution(witness) => {
            let verifier_lock = context
                .receiver_script
                .ok_or_else(|| anyhow!("receiver script not found"))?;

            let cancel: CancelChallenge<VerifyTransactionWitness> = CancelChallenge::new(
                prev_global_state,
                rollup_context,
                owner_lock,
                verifier_lock,
                witness,
            );

            let data = cancel.build_verifier_data();
            Ok(cancel.build_output(data, None))
        }
    }
}

impl<W: Entity> CancelChallenge<W> {
    pub fn new(
        prev_global_state: GlobalState,
        rollup_context: &RollupContext,
        owner_lock: Script,
        verifier_lock: Script,
        verify_witness: W,
    ) -> Self {
        let rollup_type_hash = rollup_context.rollup_script_hash.clone();

        Self {
            rollup_type_hash,
            prev_global_state,
            owner_lock,
            verifier_lock,
            verify_witness,
        }
    }

    pub fn build_output(
        self,
        verifier_data: Bytes,
        verifier_witness: Option<WitnessArgs>,
    ) -> CancelChallengeOutput {
        let verifier_size = 8 + verifier_data.len() + self.verifier_lock.as_slice().len();
        let verifier_capacity = verifier_size as u64 * 100_000_000;

        let verifier = CellOutput::new_builder()
            .capacity(verifier_capacity.pack())
            .lock(self.verifier_lock)
            .build();
        let verifier_cell = (verifier, verifier_data.into());

        let post_global_state = build_post_global_state(self.prev_global_state);
        let challenge_witness = WitnessArgs::new_builder()
            .lock(Some(self.verify_witness.as_bytes()).pack())
            .build();

        CancelChallengeOutput {
            post_global_state,
            verifier_cell,
            verifier_witness,
            challenge_witness,
            rollup_witness: build_rollup_witness(),
        }
    }
}

impl CancelChallenge<VerifyTransactionWitness> {
    fn build_verifier_data(&self) -> Bytes {
        self.owner_lock.hash().to_vec().into()
    }
}

impl CancelChallenge<VerifyTransactionSignatureWitness> {
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

        let mut hasher = new_blake2b();
        hasher.update(self.rollup_type_hash.as_slice());
        hasher.update(&self.verifier_lock.hash());
        hasher.update(receiver_script_hash.as_slice());
        hasher.update(raw_tx.as_slice());

        let mut message = [0u8; 32];
        hasher.finalize(&mut message);

        message
    }
}

impl CancelChallenge<VerifyWithdrawalWitness> {
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

        let mut hasher = new_blake2b();
        hasher.update(self.rollup_type_hash.as_slice());
        hasher.update(raw_withdrawal.as_slice());

        let mut message = [0u8; 32];
        hasher.finalize(&mut message);

        message
    }
}

fn build_post_global_state(prev_global_state: GlobalState) -> GlobalState {
    let halting_status: u8 = Status::Running.into();
    let builder = prev_global_state.clone().as_builder();
    builder.status(halting_status.into()).build()
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
