#![allow(clippy::mutable_key_type)]

use anyhow::{anyhow, Result};
use gw_common::H256;
use gw_config::ContractsCellDep;
use gw_mem_pool::{custodian::sum_withdrawals, withdrawal::Generator};
use gw_types::{
    bytes::Bytes,
    core::ScriptHashType,
    offchain::{CellInfo, CollectedCustodianCells, InputCellInfo, RollupContext},
    packed::{
        CellDep, CellInput, CellOutput, CustodianLockArgs, DepositLockArgs, L2Block, Script,
        UnlockWithdrawalViaRevert, UnlockWithdrawalWitness, UnlockWithdrawalWitnessUnion,
        WithdrawalRequestExtra, WitnessArgs,
    },
    prelude::*,
};

use std::{
    collections::HashMap,
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Clone)]
struct CkbCustodian {
    capacity: u128,
    balance: u128,
    min_capacity: u64,
}

pub struct GeneratedWithdrawals {
    pub deps: Vec<CellDep>,
    pub inputs: Vec<InputCellInfo>,
    pub outputs: Vec<(CellOutput, Bytes)>,
}

// Note: custodian lock search rollup cell in inputs
pub fn generate(
    rollup_context: &RollupContext,
    finalized_custodians: CollectedCustodianCells,
    block: &L2Block,
    contracts_dep: &ContractsCellDep,
    withdrawal_extras: &HashMap<H256, WithdrawalRequestExtra>,
) -> Result<Option<GeneratedWithdrawals>> {
    if block.withdrawals().is_empty() && finalized_custodians.cells_info.is_empty() {
        return Ok(None);
    }
    log::debug!("custodian inputs {:?}", finalized_custodians);

    let total_withdrawal_amount = sum_withdrawals(block.withdrawals().into_iter());
    let mut generator = Generator::new(rollup_context, (&finalized_custodians).into());
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
    if !total_withdrawal_amount.sudt.is_empty() || !finalized_custodians.sudt.is_empty() {
        cell_deps.push(sudt_type_dep.into());
    }

    let custodian_inputs = finalized_custodians.cells_info.into_iter().map(|cell| {
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

#[cfg(test)]
mod test {
    use std::collections::HashMap;
    use std::iter::FromIterator;

    use gw_common::{h256_ext::H256Ext, H256};
    use gw_config::BlockProducerConfig;
    use gw_types::core::ScriptHashType;
    use gw_types::offchain::{CellInfo, CollectedCustodianCells};
    use gw_types::packed::{
        Fee, L2Block, RawL2Block, RawWithdrawalRequest, Script, WithdrawalRequest,
        WithdrawalRequestExtra,
    };
    use gw_types::prelude::{Builder, Entity, Pack, PackVec, Unpack};
    use gw_types::{offchain::RollupContext, packed::RollupConfig};

    use crate::withdrawal::generate;

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
            let fee = Fee::new_builder()
                .sudt_id(20u32.pack())
                .amount(50u128.pack())
                .build();
            let raw = RawWithdrawalRequest::new_builder()
                .nonce(1u32.pack())
                .capacity((500 * 10u64.pow(8)).pack())
                .amount(20u128.pack())
                .sudt_script_hash(sudt_script.hash().pack())
                .account_script_hash(H256::from_u32(10).pack())
                .sell_amount(99999u128.pack())
                .sell_capacity(99999u64.pack())
                .owner_lock_hash(owner_lock.hash().pack())
                .payment_lock_hash(owner_lock.hash().pack())
                .fee(fee)
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

        let block_producer_config = BlockProducerConfig::default();

        // ## No owner lock
        let withdrawal_extra = WithdrawalRequestExtra::new_builder()
            .request(withdrawal.clone())
            .build();
        let withdrawal_extras = HashMap::from_iter([(withdrawal.hash().into(), withdrawal_extra)]);

        let generated = generate(
            &rollup_context,
            finalized_custodians.clone(),
            &block,
            &block_producer_config,
            &withdrawal_extras,
        )
        .unwrap();
        let (output, data) = generated.unwrap().outputs.first().unwrap().to_owned();

        let (expected_output, expected_data) =
            gw_generator::Generator::build_withdrawal_cell_output(
                &rollup_context,
                &withdrawal,
                &block.hash().into(),
                block.raw().number().unpack(),
                Some(sudt_script.clone()),
                None,
            )
            .unwrap();

        assert_eq!(expected_output.as_slice(), output.as_slice());
        assert_eq!(expected_data, data);

        // ## With owner lock
        let withdrawal_extra = WithdrawalRequestExtra::new_builder()
            .request(withdrawal.clone())
            .owner_lock(Some(owner_lock.clone()).pack())
            .build();
        let withdrawal_extras = HashMap::from_iter([(withdrawal.hash().into(), withdrawal_extra)]);

        let generated = generate(
            &rollup_context,
            finalized_custodians,
            &block,
            &block_producer_config,
            &withdrawal_extras,
        )
        .unwrap();
        let (output, data) = generated.unwrap().outputs.first().unwrap().to_owned();

        let (expected_output, expected_data) =
            gw_generator::Generator::build_withdrawal_cell_output(
                &rollup_context,
                &withdrawal,
                &block.hash().into(),
                block.raw().number().unpack(),
                Some(sudt_script.clone()),
                Some(owner_lock),
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
}
