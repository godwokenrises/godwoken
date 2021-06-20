use crate::types::CellInfo;

use anyhow::{anyhow, Result};
use ckb_types::prelude::Reader;
use ckb_types::prelude::{Builder, Entity};
use gw_chain::challenge::RevertContext;
use gw_chain::challenge::RevertWitness;
use gw_common::smt::Blake2bHasher;
use gw_common::H256;
use gw_generator::RollupContext;
use gw_types::core::Status;
use gw_types::packed::BlockMerkleState;
use gw_types::packed::ChallengeLockArgsReader;
use gw_types::packed::RawL2Block;
use gw_types::packed::RollupRevert;
use gw_types::packed::{
    CellOutput, ChallengeLockArgs, GlobalState, RollupAction, RollupActionUnion, Script,
    WitnessArgs,
};
use gw_types::prelude::Unpack;
use gw_types::{bytes::Bytes, prelude::Pack};

pub struct Revert<'a> {
    finality_blocks: u64,
    reward_burn_rate: u8,
    prev_global_state: GlobalState,
    challenge_cell: &'a CellInfo, // capacity and rewards lock
    stake_cells: &'a [CellInfo],  // calculate rewards
    burn_lock: Script,
    post_reverted_block_root: [u8; 32],
    revert_witness: RevertWitness,
}

pub struct RevertOutput {
    pub post_global_state: GlobalState,
    pub reward_cells: Vec<(CellOutput, Bytes)>,
    pub burn_cells: Vec<(CellOutput, Bytes)>,
    pub rollup_witness: WitnessArgs,
}

impl<'a> Revert<'a> {
    pub fn new(
        rollup_context: &RollupContext,
        prev_global_state: GlobalState,
        challenge_cell: &'a CellInfo,
        stake_cells: &'a [CellInfo],
        burn_lock: Script,
        revert_context: RevertContext,
    ) -> Self {
        let reward_burn_rate = rollup_context.rollup_config.reward_burn_rate().into();
        let finality_blocks = rollup_context.rollup_config.finality_blocks().unpack();

        Revert {
            finality_blocks,
            prev_global_state,
            challenge_cell,
            stake_cells,
            burn_lock,
            reward_burn_rate,
            post_reverted_block_root: revert_context.post_reverted_block_root.into(),
            revert_witness: revert_context.revert_witness,
        }
    }

    pub fn build_output(self) -> Result<RevertOutput> {
        // Rewards
        let challenge_lock_args = {
            let lock_args: Bytes = self.challenge_cell.output.lock().args().unpack();
            match ChallengeLockArgsReader::verify(&lock_args, false) {
                Ok(_) => ChallengeLockArgs::new_unchecked(lock_args),
                Err(err) => return Err(anyhow!("invalid challenge lock args {}", err)),
            }
        };
        let reward_lock = challenge_lock_args.rewards_receiver_lock();

        let rewards = Rewards::new(self.stake_cells, self.challenge_cell, self.reward_burn_rate);
        let rewards_output = rewards.build_output(reward_lock, self.burn_lock);

        // Post global state
        let first_reverted_block = {
            let blocks = &self.revert_witness.reverted_blocks;
            blocks.get(0).ok_or_else(|| anyhow!("no first block"))?
        };
        let block_merkle_state = {
            let leaves = {
                let to_leave = |b: RawL2Block| (b.smt_key().into(), H256::zero());
                let reverted_blocks = self.revert_witness.reverted_blocks.clone();
                reverted_blocks.into_iter().map(to_leave)
            };
            let block_merkle_proof = self.revert_witness.block_proof.clone();
            let block_root = block_merkle_proof.compute_root::<Blake2bHasher>(leaves.collect())?;
            let block_count = first_reverted_block.number();

            BlockMerkleState::new_builder()
                .merkle_root(block_root.pack())
                .count(block_count)
                .build()
        };
        let last_finalized_block_number = {
            let number = first_reverted_block.number().unpack();
            number
                .saturating_sub(1)
                .saturating_sub(self.finality_blocks)
        };
        let running_status: u8 = Status::Running.into();

        let post_global_state = self
            .prev_global_state
            .as_builder()
            .account(first_reverted_block.prev_account())
            .block(block_merkle_state)
            .tip_block_hash(first_reverted_block.parent_block_hash())
            .last_finalized_block_number(last_finalized_block_number.pack())
            .reverted_block_root(self.post_reverted_block_root.pack())
            .status(running_status.into())
            .build();

        // Witness
        let revert = RollupRevert::new_builder()
            .reverted_blocks(self.revert_witness.reverted_blocks)
            .block_proof(self.revert_witness.block_proof.0.pack())
            .reverted_block_proof(self.revert_witness.reverted_block_proof.0.pack())
            .build();

        let rollup_action = RollupAction::new_builder()
            .set(RollupActionUnion::RollupRevert(revert))
            .build();

        let rollup_witness = WitnessArgs::new_builder()
            .output_type(Some(rollup_action.as_bytes()).pack())
            .build();

        Ok(RevertOutput {
            post_global_state,
            reward_cells: rewards_output.reward_cells,
            burn_cells: rewards_output.burn_cells,
            rollup_witness,
        })
    }
}

struct Rewards {
    receive_capacity: u128,
    burn_capacity: u128,
}

struct RewardsOutput {
    reward_cells: Vec<(CellOutput, Bytes)>,
    burn_cells: Vec<(CellOutput, Bytes)>,
}

impl Rewards {
    fn new(stake_cells: &[CellInfo], challenge_cell: &CellInfo, reward_burn_rate: u8) -> Self {
        let to_capacity = |c: &CellInfo| c.output.capacity().unpack() as u128;

        let total_stake_capacity: u128 = stake_cells.iter().map(to_capacity).sum();
        let reward_capacity = total_stake_capacity.saturating_mul(reward_burn_rate.into()) / 100;
        let burn_capacity = total_stake_capacity.saturating_sub(reward_capacity);

        let challenge_capacity = to_capacity(challenge_cell);
        let receive_capacity = reward_capacity.saturating_add(challenge_capacity);

        Self {
            receive_capacity,
            burn_capacity,
        }
    }

    fn build_output(self, reward_lock: Script, burn_lock: Script) -> RewardsOutput {
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

        RewardsOutput {
            reward_cells: build_outputs(self.receive_capacity, reward_lock),
            burn_cells: build_outputs(self.burn_capacity, burn_lock),
        }
    }
}
