#![allow(clippy::mutable_key_type)]

use anyhow::{anyhow, bail, ensure, Context, Result};
use gw_common::{
    merkle_utils::{ckb_merkle_leaf_hash, CBMT},
    sparse_merkle_tree::CompiledMerkleProof,
    CKB_SUDT_SCRIPT_ARGS, H256,
};
use gw_config::ContractsCellDep;
use gw_mem_pool::{custodian::sum_withdrawals, withdrawal::Generator};
use gw_types::{
    bytes::Bytes,
    core::{DepType, FinalizedWithdrawalIndex, ScriptHashType},
    offchain::{
        global_state_from_slice, CellInfo, CollectedCustodianCells, InputCellInfo, RollupContext,
        WithdrawalsAmount,
    },
    packed::{
        CKBMerkleProof, CellDep, CellInput, CellOutput, CustodianLockArgs, DepositLockArgs,
        L2Block, LastFinalizedWithdrawal, RawL2BlockWithdrawals, RawL2BlockWithdrawalsVec,
        RollupFinalizeWithdrawal, Script, UnlockWithdrawalViaFinalize, UnlockWithdrawalViaRevert,
        UnlockWithdrawalWitness, UnlockWithdrawalWitnessUnion, WithdrawalRequest,
        WithdrawalRequestExtra, WitnessArgs,
    },
    prelude::*,
};
use tracing::instrument;

use std::{
    collections::HashMap,
    time::{SystemTime, UNIX_EPOCH},
};

pub mod user_withdrawal;
use self::user_withdrawal::UserWithdrawals;

pub struct BlockWithdrawals {
    block: L2Block,
    range: Option<(u32, u32)>, // start..=end
}

impl BlockWithdrawals {
    pub fn new(block: L2Block) -> Self {
        let range = if let Some(end) = Self::last_index(&block) {
            Some((0, end))
        } else {
            debug_assert!(block.withdrawals().is_empty());
            None
        };

        BlockWithdrawals { block, range }
    }

    pub fn from_rest(
        block: L2Block,
        last_finalized: &LastFinalizedWithdrawal,
    ) -> Result<Option<Self>> {
        let (finalized_bn, finalized_idx) = last_finalized.unpack_block_index();
        if finalized_bn != block.raw().number().unpack() {
            bail!("diff block and last finalized withdrawal block");
        }

        let finalized_idx_val = match finalized_idx {
            FinalizedWithdrawalIndex::NoWithdrawal | FinalizedWithdrawalIndex::AllWithdrawals => {
                return Ok(None)
            }
            FinalizedWithdrawalIndex::Value(index) => index,
        };
        ensure!(!block.withdrawals().is_empty(), "block has withdrawals");

        let last_index = Self::last_index(&block).expect("valid finalized index");
        let range = if finalized_idx_val == last_index {
            // All withdrawals are finalized but the index isnt set to `INDEX_ALL_WITHDRAWALS`.
            // In this case, we must include this block into witness to do verification
            None
        } else {
            Some((finalized_idx_val + 1, last_index))
        };

        Ok(Some(BlockWithdrawals { block, range }))
    }

    pub fn block(&self) -> &L2Block {
        &self.block
    }

    pub fn block_number(&self) -> u64 {
        self.block.raw().number().unpack()
    }

    pub fn block_range(&self) -> (u64, Option<(u32, u32)>) {
        (self.block.raw().number().unpack(), self.range)
    }

    pub fn len(&self) -> u32 {
        self.range.map(Self::count).unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        0 == self.len()
    }

    pub fn withdrawals(&self) -> impl Iterator<Item = WithdrawalRequest> {
        let (skip, take) = match self.range {
            Some((start, end)) => (start as usize, Self::count((start, end)) as usize),
            None => (0, 0),
        };

        self.block.withdrawals().into_iter().skip(skip).take(take)
    }

    pub fn withdrawal_hashes(&self) -> impl Iterator<Item = H256> {
        self.withdrawals().map(|w| w.hash().into())
    }

    pub fn generate_witness(&self) -> Result<RawL2BlockWithdrawals> {
        let offset = match self.range {
            Some((offset, _)) => offset,
            None => {
                return Ok(RawL2BlockWithdrawals::new_builder()
                    .raw_l2block(self.block.raw())
                    .build())
            }
        };

        let leaves = { self.block.withdrawals().into_iter().enumerate() }
            .map(|(i, w)| {
                let hash: H256 = w.witness_hash().into();
                ckb_merkle_leaf_hash(i as u32, &hash)
            })
            .collect::<Vec<_>>();

        let (indices, proof_withdrawals): (Vec<_>, Vec<_>) = { self.withdrawals().enumerate() }
            .map(|(i, w)| (i as u32 + offset, w))
            .unzip();

        let proof = CBMT::build_merkle_proof(&leaves, &indices).with_context(|| {
            let block_number = self.block.raw().number().unpack();
            format!("block {} range {:?}", block_number, self.range)
        })?;
        let cbmt_proof = CKBMerkleProof::new_builder()
            .lemmas(proof.lemmas().pack())
            .indices(proof.indices().pack())
            .build();

        let block_withdrawals = RawL2BlockWithdrawals::new_builder()
            .raw_l2block(self.block.raw())
            .withdrawals(proof_withdrawals.pack())
            .withdrawal_proof(cbmt_proof)
            .build();

        Ok(block_withdrawals)
    }

    pub fn take(self, n: u32) -> Option<Self> {
        let range = self.range?;
        let count = Self::count(range);
        if count < n {
            return None;
        }
        if count == n {
            return Some(self);
        }

        let (start, _) = range;
        let taken = BlockWithdrawals {
            block: self.block,
            range: Some((start, start + n - 1)),
        };

        Some(taken)
    }

    pub fn to_last_finalized_withdrawal(&self) -> LastFinalizedWithdrawal {
        let index = match self.range {
            None => LastFinalizedWithdrawal::INDEX_NO_WITHDRAWAL,
            Some((_, end)) if Some(end) == Self::last_index(&self.block) => {
                LastFinalizedWithdrawal::INDEX_ALL_WITHDRAWALS
            }
            Some((_, end)) => end,
        };

        LastFinalizedWithdrawal::new_builder()
            .block_number(self.block.raw().number())
            .withdrawal_index(index.pack())
            .build()
    }

    fn last_index(block: &L2Block) -> Option<u32> {
        (block.withdrawals().len() as u32).checked_sub(1)
    }

    fn count((start, end): (u32, u32)) -> u32 {
        end - start + 1 // +1 for inclusive end, aka start..=end
    }
}

pub struct FinalizedWithdrawals {
    pub withdrawals: Option<(WithdrawalsAmount, Vec<(CellOutput, Bytes)>)>,
    pub witness: RollupFinalizeWithdrawal,
}

#[instrument(skip_all)]
pub fn finalize(
    block_withdrawals: &[BlockWithdrawals],
    block_proof: CompiledMerkleProof,
    extra_map: &HashMap<H256, WithdrawalRequestExtra>,
    sudt_script_map: &HashMap<H256, Script>,
) -> Result<FinalizedWithdrawals> {
    let mut withdrawals = None;

    if block_withdrawals.iter().any(|bw| !bw.is_empty()) {
        let extras = { block_withdrawals.iter() }
            .flat_map(|bw| {
                bw.withdrawal_hashes().map(|h| {
                    { extra_map.get(&h) }
                        .ok_or_else(|| anyhow!("withdrawal extra {} not found", h.pack()))
                })
            })
            .collect::<Result<Vec<_>>>()?;

        let aggregated = aggregate_withdrawals(extras, sudt_script_map)?;

        let user_withdrawal_outputs = { aggregated.users.into_values() }
            .filter_map(UserWithdrawals::into_outputs)
            .flatten()
            .collect();

        withdrawals = Some((aggregated.total, user_withdrawal_outputs));
    }

    let withdrawals_witness = { block_withdrawals.iter() }
        .map(BlockWithdrawals::generate_witness)
        .collect::<Result<Vec<_>>>()?;

    let witness = RollupFinalizeWithdrawal::new_builder()
        .block_withdrawals(
            RawL2BlockWithdrawalsVec::new_builder()
                .set(withdrawals_witness)
                .build(),
        )
        .block_proof(block_proof.0.pack())
        .build();

    let finalized = FinalizedWithdrawals {
        withdrawals,
        witness,
    };

    Ok(finalized)
}

pub struct GeneratedWithdrawals {
    pub deps: Vec<CellDep>,
    pub inputs: Vec<InputCellInfo>,
    pub outputs: Vec<(CellOutput, Bytes)>,
}

// Note: custodian lock search rollup cell in inputs
// TODO: remove after enable finalize withdrawals
pub fn generate(
    rollup_context: &RollupContext,
    mut finalized_custodians: CollectedCustodianCells,
    block: &L2Block,
    contracts_dep: &ContractsCellDep,
    withdrawal_extras: &HashMap<H256, WithdrawalRequestExtra>,
) -> Result<Option<GeneratedWithdrawals>> {
    if block.withdrawals().is_empty() && finalized_custodians.cells_info.len() <= 1 {
        return Ok(None);
    }
    log::debug!("custodian inputs {:?}", finalized_custodians);

    let cells_info = std::mem::take(&mut finalized_custodians.cells_info);
    let cusotidan_sudt_is_empty = finalized_custodians.sudt.is_empty();

    let total_withdrawal_amount = sum_withdrawals(block.withdrawals().into_iter());
    let mut generator = Generator::new(rollup_context, finalized_custodians.into());
    for req in block.withdrawals().into_iter() {
        let req_extra = match withdrawal_extras.get(&req.hash().into()) {
            Some(req_extra) => req_extra.to_owned(),
            None => WithdrawalRequestExtra::new_builder().request(req).build(),
        };
        generator
            .include_and_verify(&req_extra, block)
            .map_err(|err| anyhow!("unexpected withdrawal err {}", err))?
    }
    log::debug!("included withdrawals {}", generator.withdrawals().len());

    let custodian_lock_dep = contracts_dep.custodian_cell_lock.clone();
    let sudt_type_dep = contracts_dep.l1_sudt_type.clone();
    let mut cell_deps = vec![custodian_lock_dep.into()];
    if !total_withdrawal_amount.sudt.is_empty() || !cusotidan_sudt_is_empty {
        cell_deps.push(sudt_type_dep.into());
    }

    let custodian_inputs = cells_info.into_iter().map(|cell| {
        let input = CellInput::new_builder()
            .previous_output(cell.out_point.clone())
            .build();
        InputCellInfo { input, cell }
    });

    let generated_withdrawals = GeneratedWithdrawals {
        deps: cell_deps,
        inputs: custodian_inputs.collect(),
        outputs: generator.finish(),
    };

    Ok(Some(generated_withdrawals))
}

pub struct RevertedWithdrawals {
    pub deps: Vec<CellDep>,
    pub inputs: Vec<InputCellInfo>,
    pub witness_args: Vec<WitnessArgs>,
    pub outputs: Vec<(CellOutput, Bytes)>,
}

// TODO: remove after enable finalize withdrawals
pub fn revert(
    rollup_context: &RollupContext,
    contracts_dep: &ContractsCellDep,
    withdrawal_cells: Vec<CellInfo>,
) -> Result<Option<RevertedWithdrawals>> {
    if withdrawal_cells.is_empty() {
        return Ok(None);
    }

    let mut withdrawal_inputs = vec![];
    let mut withdrawal_witness = vec![];
    let mut custodian_outputs = vec![];

    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("unexpected timestamp")
        .as_millis() as u64;

    // We use timestamp plus idx and rollup_type_hash to create different custodian lock
    // hash for every reverted withdrawal input. Withdrawal lock use custodian lock hash to
    // index corresponding custodian output.
    // NOTE: These locks must also be different from custodian change cells created by
    // withdrawal requests processing.
    let rollup_type_hash = rollup_context.rollup_script_hash.as_slice().iter();
    for (idx, withdrawal) in withdrawal_cells.into_iter().enumerate() {
        let custodian_lock = {
            let deposit_lock_args = DepositLockArgs::new_builder()
                .owner_lock_hash(rollup_context.rollup_script_hash.pack())
                .cancel_timeout((idx as u64 + timestamp).pack())
                .build();

            let custodian_lock_args = CustodianLockArgs::new_builder()
                .deposit_lock_args(deposit_lock_args)
                .build();

            let lock_args: Bytes = rollup_type_hash
                .clone()
                .chain(custodian_lock_args.as_slice().iter())
                .cloned()
                .collect();

            Script::new_builder()
                .code_hash(rollup_context.rollup_config.custodian_script_type_hash())
                .hash_type(ScriptHashType::Type.into())
                .args(lock_args.pack())
                .build()
        };

        let custodian_output = {
            let output_builder = withdrawal.output.clone().as_builder();
            output_builder.lock(custodian_lock.clone()).build()
        };

        let withdrawal_input = {
            let input = CellInput::new_builder()
                .previous_output(withdrawal.out_point.clone())
                .build();

            InputCellInfo {
                input,
                cell: withdrawal.clone(),
            }
        };

        let unlock_withdrawal_witness = {
            let unlock_withdrawal_via_revert = UnlockWithdrawalViaRevert::new_builder()
                .custodian_lock_hash(custodian_lock.hash().pack())
                .build();

            UnlockWithdrawalWitness::new_builder()
                .set(UnlockWithdrawalWitnessUnion::UnlockWithdrawalViaRevert(
                    unlock_withdrawal_via_revert,
                ))
                .build()
        };
        let withdrawal_witness_args = WitnessArgs::new_builder()
            .lock(Some(unlock_withdrawal_witness.as_bytes()).pack())
            .build();

        withdrawal_inputs.push(withdrawal_input);
        withdrawal_witness.push(withdrawal_witness_args);
        custodian_outputs.push((custodian_output, withdrawal.data.clone()));
    }

    let withdrawal_lock_dep = contracts_dep.withdrawal_cell_lock.clone();
    let sudt_type_dep = contracts_dep.l1_sudt_type.clone();
    let mut cell_deps = vec![withdrawal_lock_dep.into()];
    if withdrawal_inputs
        .iter()
        .any(|info| info.cell.output.type_().to_opt().is_some())
    {
        cell_deps.push(sudt_type_dep.into())
    }

    Ok(Some(RevertedWithdrawals {
        deps: cell_deps,
        inputs: withdrawal_inputs,
        outputs: custodian_outputs,
        witness_args: withdrawal_witness,
    }))
}

pub struct UnlockedWithdrawals {
    pub deps: Vec<CellDep>,
    pub inputs: Vec<InputCellInfo>,
    pub witness_args: Vec<WitnessArgs>,
    pub outputs: Vec<(CellOutput, Bytes)>,
}

// TODO: remove after enable finalize withdrawals
pub fn unlock_to_owner(
    rollup_cell: CellInfo,
    rollup_context: &RollupContext,
    contracts_dep: &ContractsCellDep,
    withdrawal_cells: Vec<CellInfo>,
) -> Result<Option<UnlockedWithdrawals>> {
    if withdrawal_cells.is_empty() {
        return Ok(None);
    }

    let mut withdrawal_inputs = vec![];
    let mut withdrawal_witness = vec![];
    let mut unlocked_to_owner_outputs = vec![];

    let unlock_via_finalize_witness = {
        let unlock_args = UnlockWithdrawalViaFinalize::new_builder().build();
        let unlock_witness = UnlockWithdrawalWitness::new_builder()
            .set(UnlockWithdrawalWitnessUnion::UnlockWithdrawalViaFinalize(
                unlock_args,
            ))
            .build();
        WitnessArgs::new_builder()
            .lock(Some(unlock_witness.as_bytes()).pack())
            .build()
    };

    let global_state = global_state_from_slice(&rollup_cell.data)?;
    let last_finalized_block_number: u64 = global_state.last_finalized_block_number().unpack();
    let l1_sudt_script_hash = rollup_context.rollup_config.l1_sudt_script_type_hash();
    for withdrawal_cell in withdrawal_cells {
        // Double check
        if let Err(err) = gw_rpc_client::withdrawal::verify_unlockable_to_owner(
            &withdrawal_cell,
            last_finalized_block_number,
            &l1_sudt_script_hash,
        ) {
            log::error!("[unlock withdrawal] unexpected verify failed {}", err);
            continue;
        }

        let owner_lock = {
            let args: Bytes = withdrawal_cell.output.lock().args().unpack();
            match gw_utils::withdrawal::parse_lock_args(&args) {
                Ok(parsed) => parsed.owner_lock,
                Err(_) => {
                    log::error!("[unlock withdrawal] impossible, already pass verify_unlockable_to_owner above");
                    continue;
                }
            }
        };

        let withdrawal_input = {
            let input = CellInput::new_builder()
                .previous_output(withdrawal_cell.out_point.clone())
                .build();

            InputCellInfo {
                input,
                cell: withdrawal_cell.clone(),
            }
        };

        // Switch to owner lock
        let output = withdrawal_cell.output.as_builder().lock(owner_lock).build();

        withdrawal_inputs.push(withdrawal_input);
        withdrawal_witness.push(unlock_via_finalize_witness.clone());
        unlocked_to_owner_outputs.push((output, withdrawal_cell.data));
    }

    if withdrawal_inputs.is_empty() {
        return Ok(None);
    }

    let rollup_dep = CellDep::new_builder()
        .out_point(rollup_cell.out_point)
        .dep_type(DepType::Code.into())
        .build();
    let withdrawal_lock_dep = contracts_dep.withdrawal_cell_lock.clone();
    let sudt_type_dep = contracts_dep.l1_sudt_type.clone();

    let mut cell_deps = vec![rollup_dep, withdrawal_lock_dep.into()];
    if unlocked_to_owner_outputs
        .iter()
        .any(|output| output.0.type_().to_opt().is_some())
    {
        cell_deps.push(sudt_type_dep.into())
    }

    Ok(Some(UnlockedWithdrawals {
        deps: cell_deps,
        inputs: withdrawal_inputs,
        witness_args: withdrawal_witness,
        outputs: unlocked_to_owner_outputs,
    }))
}

struct AggregatedWithdrawals {
    total: WithdrawalsAmount,
    users: HashMap<H256, UserWithdrawals>,
}

fn aggregate_withdrawals<'a>(
    extras: impl IntoIterator<Item = &'a WithdrawalRequestExtra>,
    sudt_scripts: &HashMap<H256, Script>,
) -> Result<AggregatedWithdrawals> {
    let mut total = WithdrawalsAmount::default();
    let mut users = HashMap::new();

    for extra in extras {
        let raw = extra.request().raw();

        total.capacity = { total.capacity }
            .checked_add(raw.capacity().unpack().into())
            .context("capacity overflow")?;

        let owner_lock = extra.owner_lock();
        let user_mut = users
            .entry(owner_lock.hash().into())
            .or_insert_with(|| UserWithdrawals::new(owner_lock));

        let sudt_amount = raw.amount().unpack();
        if 0 == sudt_amount {
            user_mut.push_extra((extra, None))?;
            continue;
        }

        let sudt_script_hash: [u8; 32] = raw.sudt_script_hash().unpack();
        if CKB_SUDT_SCRIPT_ARGS == sudt_script_hash {
            bail!("invalid sudt withdrawal {:x}", raw.hash().pack());
        }

        let sudt_balance_mut = total.sudt.entry(sudt_script_hash).or_insert(0);
        *sudt_balance_mut = sudt_balance_mut
            .checked_add(sudt_amount)
            .with_context(|| ckb_fixed_hash::H256(raw.hash()))?;

        let sudt_script = sudt_scripts
            .get(&sudt_script_hash.into())
            .with_context(|| ckb_fixed_hash::H256(raw.hash()))?;

        user_mut.push_extra((extra, Some((*sudt_script).to_owned())))?;
    }

    Ok(AggregatedWithdrawals { total, users })
}

#[cfg(test)]
mod test {
    use std::collections::HashMap;
    use std::iter::FromIterator;

    use gw_common::{h256_ext::H256Ext, H256};
    use gw_config::ContractsCellDep;
    use gw_types::core::{DepType, ScriptHashType};
    use gw_types::offchain::{CellInfo, CollectedCustodianCells, InputCellInfo};
    use gw_types::packed::{
        CellDep, CellInput, CellOutput, GlobalState, L2Block, OutPoint, RawL2Block,
        RawWithdrawalRequest, Script, UnlockWithdrawalViaFinalize, UnlockWithdrawalWitness,
        UnlockWithdrawalWitnessUnion, WithdrawalLockArgs, WithdrawalRequest,
        WithdrawalRequestExtra, WitnessArgs,
    };
    use gw_types::prelude::{Builder, Entity, Pack, PackVec, Unpack};
    use gw_types::{offchain::RollupContext, packed::RollupConfig};

    use crate::withdrawal::generate;

    use super::unlock_to_owner;

    #[test]
    fn test_withdrawal_cell_generate() {
        let rollup_context = RollupContext {
            rollup_script_hash: H256::from_u32(1),
            rollup_config: RollupConfig::new_builder()
                .withdrawal_script_type_hash(H256::from_u32(100).pack())
                .finality_blocks(1u64.pack())
                .build(),
        };

        let sudt_script = Script::new_builder()
            .code_hash(H256::from_u32(2).pack())
            .hash_type(ScriptHashType::Type.into())
            .args(vec![3u8; 32].pack())
            .build();

        let finalized_custodians = CollectedCustodianCells {
            cells_info: vec![CellInfo::default()],
            capacity: u64::MAX as u128,
            sudt: HashMap::from_iter([(sudt_script.hash(), (u128::MAX, sudt_script.clone()))]),
        };

        let owner_lock = Script::new_builder()
            .code_hash(H256::from_u32(4).pack())
            .args(vec![5; 32].pack())
            .build();

        let withdrawal = {
            let fee = 50u128;
            let raw = RawWithdrawalRequest::new_builder()
                .nonce(1u32.pack())
                .capacity((500 * 10u64.pow(8)).pack())
                .amount(20u128.pack())
                .sudt_script_hash(sudt_script.hash().pack())
                .account_script_hash(H256::from_u32(10).pack())
                .owner_lock_hash(owner_lock.hash().pack())
                .fee(fee.pack())
                .build();
            WithdrawalRequest::new_builder()
                .raw(raw)
                .signature(vec![6u8; 65].pack())
                .build()
        };

        let raw_block = RawL2Block::new_builder().number(1000u64.pack()).build();
        let block = L2Block::new_builder()
            .raw(raw_block)
            .withdrawals(vec![withdrawal.clone()].pack())
            .build();

        let contracts_dep = ContractsCellDep::default();

        // ## With owner lock
        let withdrawal_extra = WithdrawalRequestExtra::new_builder()
            .request(withdrawal.clone())
            .owner_lock(owner_lock)
            .build();
        let withdrawal_extras =
            HashMap::from_iter([(withdrawal.hash().into(), withdrawal_extra.clone())]);

        let generated = generate(
            &rollup_context,
            finalized_custodians,
            &block,
            &contracts_dep,
            &withdrawal_extras,
        )
        .unwrap();
        let (output, data) = generated.unwrap().outputs.first().unwrap().to_owned();

        let (expected_output, expected_data) = gw_generator::utils::build_withdrawal_cell_output(
            &rollup_context,
            &withdrawal_extra,
            &block.hash().into(),
            block.raw().number().unpack(),
            Some(sudt_script.clone()),
        )
        .unwrap();

        assert_eq!(expected_output.as_slice(), output.as_slice());
        assert_eq!(expected_data, data);

        // Check our generate withdrawal can be queried and unlocked to owner
        let info = CellInfo {
            output,
            data,
            ..Default::default()
        };
        let last_finalized_block_number =
            block.raw().number().unpack() + rollup_context.rollup_config.finality_blocks().unpack();
        gw_rpc_client::withdrawal::verify_unlockable_to_owner(
            &info,
            last_finalized_block_number,
            &sudt_script.code_hash(),
        )
        .expect("pass verification");
    }

    #[test]
    fn test_unlock_to_owner() {
        // Output should only change lock to owner lock
        let last_finalized_block_number = 100u64;
        let global_state = GlobalState::new_builder()
            .last_finalized_block_number(last_finalized_block_number.pack())
            .build();

        let rollup_type = Script::new_builder()
            .code_hash(H256::from_u32(1).pack())
            .build();

        let rollup_cell = CellInfo {
            data: global_state.as_bytes(),
            out_point: OutPoint::new_builder()
                .tx_hash(H256::from_u32(2).pack())
                .build(),
            output: CellOutput::new_builder()
                .type_(Some(rollup_type.clone()).pack())
                .build(),
        };

        let sudt_script = Script::new_builder()
            .code_hash(H256::from_u32(3).pack())
            .hash_type(ScriptHashType::Type.into())
            .args(vec![4u8; 32].pack())
            .build();

        let rollup_context = RollupContext {
            rollup_script_hash: rollup_type.hash().into(),
            rollup_config: RollupConfig::new_builder()
                .withdrawal_script_type_hash(H256::from_u32(5).pack())
                .l1_sudt_script_type_hash(sudt_script.code_hash())
                .finality_blocks(1u64.pack())
                .build(),
        };

        let contracts_dep = {
            let withdrawal_out_point = OutPoint::new_builder()
                .tx_hash(H256::from_u32(6).pack())
                .build();
            let l1_sudt_out_point = OutPoint::new_builder()
                .tx_hash(H256::from_u32(7).pack())
                .build();

            ContractsCellDep {
                withdrawal_cell_lock: CellDep::new_builder()
                    .out_point(withdrawal_out_point)
                    .build()
                    .into(),
                l1_sudt_type: CellDep::new_builder()
                    .out_point(l1_sudt_out_point)
                    .build()
                    .into(),
                ..Default::default()
            }
        };

        let owner_lock = Script::new_builder()
            .code_hash(H256::from_u32(8).pack())
            .hash_type(ScriptHashType::Type.into())
            .args(vec![9u8; 32].pack())
            .build();

        let withdrawal_without_owner_lock = {
            let lock_args = WithdrawalLockArgs::new_builder()
                .owner_lock_hash(owner_lock.hash().pack())
                .withdrawal_block_number((last_finalized_block_number - 1).pack())
                .build();

            let mut args = rollup_type.hash().to_vec();
            args.extend_from_slice(&lock_args.as_bytes());

            let lock = Script::new_builder().args(args.pack()).build();
            CellInfo {
                output: CellOutput::new_builder().lock(lock).build(),
                ..Default::default()
            }
        };

        let withdrawal_with_owner_lock = {
            let lock_args = WithdrawalLockArgs::new_builder()
                .owner_lock_hash(owner_lock.hash().pack())
                .withdrawal_block_number((last_finalized_block_number - 1).pack())
                .build();

            let mut args = rollup_type.hash().to_vec();
            args.extend_from_slice(&lock_args.as_bytes());
            args.extend_from_slice(&(owner_lock.as_bytes().len() as u32).to_be_bytes());
            args.extend_from_slice(&owner_lock.as_bytes());

            let lock = Script::new_builder().args(args.pack()).build();
            CellInfo {
                output: CellOutput::new_builder()
                    .type_(Some(sudt_script).pack())
                    .lock(lock)
                    .build(),
                data: 100u128.pack().as_bytes(),
                ..Default::default()
            }
        };

        let unlocked = unlock_to_owner(
            rollup_cell.clone(),
            &rollup_context,
            &contracts_dep,
            vec![
                withdrawal_without_owner_lock,
                withdrawal_with_owner_lock.clone(),
            ],
        )
        .expect("unlock")
        .expect("some unlocked");

        assert_eq!(unlocked.inputs.len(), 1, "skip one without owner lock");
        assert_eq!(unlocked.outputs.len(), 1);
        assert_eq!(unlocked.witness_args.len(), 1);

        let expected_output = {
            let output = withdrawal_with_owner_lock.output.clone().as_builder();
            output.lock(owner_lock).build()
        };

        let (output, data) = unlocked.outputs.first().unwrap().to_owned();
        assert_eq!(expected_output.as_slice(), output.as_slice());
        assert_eq!(withdrawal_with_owner_lock.data, data);

        let expected_input = {
            let input = CellInput::new_builder()
                .previous_output(withdrawal_with_owner_lock.out_point.clone())
                .build();

            InputCellInfo {
                input,
                cell: withdrawal_with_owner_lock,
            }
        };
        let input = unlocked.inputs.first().unwrap().to_owned();
        assert_eq!(expected_input.input.as_slice(), input.input.as_slice());
        assert_eq!(
            expected_input.cell.output.as_slice(),
            input.cell.output.as_slice()
        );
        assert_eq!(
            expected_input.cell.out_point.as_slice(),
            input.cell.out_point.as_slice()
        );
        assert_eq!(expected_input.cell.data, input.cell.data);

        let expected_witness = {
            let unlock_args = UnlockWithdrawalViaFinalize::new_builder().build();
            let unlock_witness = UnlockWithdrawalWitness::new_builder()
                .set(UnlockWithdrawalWitnessUnion::UnlockWithdrawalViaFinalize(
                    unlock_args,
                ))
                .build();
            WitnessArgs::new_builder()
                .lock(Some(unlock_witness.as_bytes()).pack())
                .build()
        };
        let witness = unlocked.witness_args.first().unwrap().to_owned();
        assert_eq!(expected_witness.as_slice(), witness.as_slice());

        assert_eq!(unlocked.deps.len(), 3);
        let rollup_dep = CellDep::new_builder()
            .out_point(rollup_cell.out_point)
            .dep_type(DepType::Code.into())
            .build();
        assert_eq!(
            unlocked.deps.first().unwrap().as_slice(),
            rollup_dep.as_slice()
        );
        assert_eq!(
            unlocked.deps.get(1).unwrap().as_slice(),
            CellDep::from(contracts_dep.withdrawal_cell_lock).as_slice(),
        );
        assert_eq!(
            unlocked.deps.get(2).unwrap().as_slice(),
            CellDep::from(contracts_dep.l1_sudt_type).as_slice(),
        );
    }
}
