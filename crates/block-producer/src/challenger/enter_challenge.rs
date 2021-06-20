use ckb_types::prelude::{Builder, Entity};
use gw_common::H256;
use gw_generator::{ChallengeContext, RollupContext};
use gw_types::core::{ScriptHashType, Status};
use gw_types::packed::{
    Byte32, CellOutput, ChallengeLockArgs, ChallengeTarget, ChallengeWitness, GlobalState,
    RollupAction, RollupActionUnion, RollupEnterChallenge, Script, WitnessArgs,
};
use gw_types::{bytes::Bytes, prelude::Pack};

pub struct EnterChallenge {
    rollup_type_hash: H256,
    challenge_script_type_hash: Byte32,
    prev_global_state: GlobalState,
    target: ChallengeTarget,
    witness: ChallengeWitness,
    rewards_lock: Script,
}

pub struct EnterChallengeOutput {
    pub post_global_state: GlobalState,
    pub challenge_cell: (CellOutput, Bytes),
    pub rollup_witness: WitnessArgs,
}

impl EnterChallenge {
    pub fn new(
        prev_global_state: GlobalState,
        rollup_context: &RollupContext,
        challenge_context: ChallengeContext,
        rewards_lock: Script,
    ) -> Self {
        let rollup_type_hash = rollup_context.rollup_script_hash.clone();
        let challenge_script_type_hash = rollup_context.rollup_config.challenge_script_type_hash();

        EnterChallenge {
            rollup_type_hash,
            challenge_script_type_hash,
            prev_global_state,
            target: challenge_context.target,
            witness: challenge_context.witness,
            rewards_lock,
        }
    }

    pub fn build_output(self) -> EnterChallengeOutput {
        let lock_args: Bytes = {
            let challenge_lock_args = ChallengeLockArgs::new_builder()
                .target(self.target)
                .rewards_receiver_lock(self.rewards_lock)
                .build();

            let rollup_type_hash = self.rollup_type_hash.as_slice().iter();
            rollup_type_hash
                .chain(challenge_lock_args.as_slice().iter())
                .cloned()
                .collect()
        };

        let challenge_lock = Script::new_builder()
            .code_hash(self.challenge_script_type_hash)
            .hash_type(ScriptHashType::Type.into())
            .args(lock_args.pack())
            .build();

        let size = 8 + challenge_lock.as_slice().len();
        let capacity = size as u64 * 100_000_000;

        let challenge = CellOutput::new_builder()
            .capacity(capacity.pack())
            .lock(challenge_lock)
            .build();
        let challenge_cell = (challenge, Bytes::default());

        let halting_status: u8 = Status::Halting.into();
        let post_global_state = {
            let builder = self.prev_global_state.clone().as_builder();
            builder.status(halting_status.into()).build()
        };

        // Build witness
        let enter_challenge = RollupEnterChallenge::new_builder()
            .witness(self.witness.clone())
            .build();

        let rollup_action = RollupAction::new_builder()
            .set(RollupActionUnion::RollupEnterChallenge(enter_challenge))
            .build();

        let rollup_witness = WitnessArgs::new_builder()
            .output_type(Some(rollup_action.as_bytes()).pack())
            .build();

        EnterChallengeOutput {
            post_global_state,
            challenge_cell,
            rollup_witness,
        }
    }
}
