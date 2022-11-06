use std::{collections::HashSet, time::Instant};

use anyhow::{anyhow, bail, Result};
use gw_common::{CKB_SUDT_SCRIPT_ARGS, H256};
use gw_rpc_client::{
    indexer_client::CKBIndexerClient,
    indexer_types::{Order, SearchKey, SearchKeyFilter},
    rpc_client::{QueryResult, RPCClient},
};
use gw_store::traits::chain_store::ChainStore;
use gw_types::core::Timepoint;
use gw_types::offchain::CompatibleFinalizedTimepoint;
use gw_types::{
    bytes::Bytes,
    core::ScriptHashType,
    offchain::{CellInfo, CollectedCustodianCells, DepositInfo, WithdrawalsAmount},
    packed::{
        CellOutput, CustodianLockArgs, CustodianLockArgsReader, DepositLockArgs, Script,
        WithdrawalRequest,
    },
    prelude::*,
};
use gw_utils::local_cells::{
    collect_local_and_indexer_cells, CollectLocalAndIndexerCursor, LocalCellsManager,
};
use gw_utils::RollupContext;
use tracing::instrument;

use crate::constants::MAX_CUSTODIANS;

pub fn to_custodian_cell(
    rollup_context: &RollupContext,
    block_hash: &H256,
    block_timepoint: &Timepoint,
    deposit_info: &DepositInfo,
) -> Result<(CellOutput, Bytes), u128> {
    let lock_args: Bytes = {
        let deposit_lock_args = {
            let lock_args: Bytes = deposit_info.cell.output.lock().args().unpack();
            DepositLockArgs::new_unchecked(lock_args.slice(32..))
        };

        let custodian_lock_args = CustodianLockArgs::new_builder()
            .deposit_block_hash(Into::<[u8; 32]>::into(*block_hash).pack())
            .deposit_block_number(block_timepoint.full_value().pack())
            .deposit_lock_args(deposit_lock_args)
            .build();

        let rollup_type_hash = rollup_context.rollup_script_hash.as_slice().iter();
        rollup_type_hash
            .chain(custodian_lock_args.as_slice().iter())
            .cloned()
            .collect()
    };
    let lock = Script::new_builder()
        .code_hash(rollup_context.rollup_config.custodian_script_type_hash())
        .hash_type(ScriptHashType::Type.into())
        .args(lock_args.pack())
        .build();

    // Use custodian lock
    let output = {
        let builder = deposit_info.cell.output.clone().as_builder();
        builder.lock(lock).build()
    };
    let data = deposit_info.cell.data.clone();

    // Check capacity
    match output.occupied_capacity(data.len()) {
        Ok(capacity) if capacity > deposit_info.cell.output.capacity().unpack() => {
            return Err(capacity as u128);
        }
        // Overflow
        Err(err) => {
            log::debug!("calculate occupied capacity {}", err);
            return Err(u64::MAX as u128 + 1);
        }
        _ => (),
    }

    Ok((output, data))
}

pub fn sum_withdrawals<Iter: Iterator<Item = WithdrawalRequest>>(reqs: Iter) -> WithdrawalsAmount {
    reqs.fold(
        WithdrawalsAmount::default(),
        |mut total_amount, withdrawal| {
            total_amount.capacity = total_amount
                .capacity
                .saturating_add(withdrawal.raw().capacity().unpack() as u128);

            let sudt_script_hash = withdrawal.raw().sudt_script_hash().unpack();
            let sudt_amount = withdrawal.raw().amount().unpack();
            if sudt_amount != 0 {
                if sudt_script_hash ==
                    CKB_SUDT_SCRIPT_ARGS {
                        let account = withdrawal.raw().account_script_hash();
                        log::warn!("{} withdrawal request non-zero sudt amount but it's type hash ckb, ignore this amount", account);
                    }
                    else{
                        let total_sudt_amount = total_amount.sudt.entry(sudt_script_hash).or_insert(0u128);
                        *total_sudt_amount = total_sudt_amount.saturating_add(sudt_amount);
                    }
            }

            total_amount
        }
    )
}

pub async fn query_finalized_custodians<WithdrawalIter: Iterator<Item = WithdrawalRequest>>(
    rpc_client: &RPCClient,
    db: &impl ChainStore,
    withdrawals: WithdrawalIter,
    rollup_context: &RollupContext,
    compatible_finalized_timepoint: &CompatibleFinalizedTimepoint,
    local_cells_manager: &LocalCellsManager,
) -> Result<QueryResult<CollectedCustodianCells>> {
    let total_withdrawal_amount = sum_withdrawals(withdrawals);
    let total_change_capacity = sum_change_capacity(db, rollup_context, &total_withdrawal_amount);

    query_finalized_custodian_cells(
        local_cells_manager,
        &rpc_client.indexer,
        rollup_context,
        &total_withdrawal_amount,
        total_change_capacity,
        compatible_finalized_timepoint,
        None,
        MAX_CUSTODIANS,
    )
    .await
}

pub fn calc_ckb_custodian_min_capacity(rollup_context: &RollupContext) -> u64 {
    let lock = build_finalized_custodian_lock(rollup_context);
    let dummy = CellOutput::new_builder()
        .capacity(1u64.pack())
        .lock(lock)
        .build();
    dummy.occupied_capacity(0).expect("overflow")
}

pub fn build_finalized_custodian_lock(rollup_context: &RollupContext) -> Script {
    let rollup_type_hash = rollup_context.rollup_script_hash.as_slice().iter();
    let custodian_lock_args = CustodianLockArgs::default();

    let args: Bytes = rollup_type_hash
        .chain(custodian_lock_args.as_slice().iter())
        .cloned()
        .collect();

    Script::new_builder()
        .code_hash(rollup_context.rollup_config.custodian_script_type_hash())
        .hash_type(ScriptHashType::Type.into())
        .args(args.pack())
        .build()
}

pub fn generate_finalized_custodian(
    rollup_context: &RollupContext,
    amount: u128,
    type_: Script,
) -> (CellOutput, Bytes) {
    let lock = build_finalized_custodian_lock(rollup_context);
    let data = amount.pack().as_bytes();
    let dummy_capacity = 1;
    let output = CellOutput::new_builder()
        .capacity(dummy_capacity.pack())
        .type_(Some(type_).pack())
        .lock(lock)
        .build();
    let capacity = output.occupied_capacity(data.len()).expect("overflow");
    let output = output.as_builder().capacity(capacity.pack()).build();

    (output, data)
}

#[instrument(skip_all, fields(withdrawals_amount = ?withdrawals_amount))]
fn sum_change_capacity(
    db: &impl ChainStore,
    rollup_context: &RollupContext,
    withdrawals_amount: &WithdrawalsAmount,
) -> u128 {
    let to_change_capacity = |sudt_script_hash: &[u8; 32]| -> u128 {
        match db.get_asset_script(&H256::from(*sudt_script_hash)) {
            Ok(Some(script)) => {
                let (change, _data) = generate_finalized_custodian(rollup_context, 1, script);
                change.capacity().unpack() as u128
            }
            _ => {
                let hex = hex::encode(&sudt_script_hash);
                log::warn!("unknown sudt script hash {:?}", hex);
                0
            }
        }
    };

    let ckb_change_capacity = calc_ckb_custodian_min_capacity(rollup_context) as u128;
    let sudt_change_capacity: u128 = {
        let sudt_script_hashes = withdrawals_amount.sudt.keys();
        sudt_script_hashes.map(to_change_capacity).sum()
    };

    ckb_change_capacity + sudt_change_capacity
}

#[allow(clippy::too_many_arguments)]
async fn query_finalized_custodian_cells(
    local_cells_manager: &LocalCellsManager,
    indexer: &CKBIndexerClient,
    rollup_context: &RollupContext,
    withdrawals_amount: &WithdrawalsAmount,
    custodian_change_capacity: u128,
    compatible_finalized_timepoint: &CompatibleFinalizedTimepoint,
    min_capacity: Option<u64>,
    max_cells: usize,
) -> Result<QueryResult<CollectedCustodianCells>> {
    const MAX_CELLS: usize = 50;

    let mut query_indexer_times = 0;
    let mut query_indexer_cells = 0;
    let now = Instant::now();

    let parse_sudt_amount = |cell: &CellInfo| -> Result<u128> {
        if cell.output.type_().is_none() {
            bail!("no a sudt cell");
        }

        gw_types::packed::Uint128::from_slice(cell.data.as_ref())
            .map(|a| a.unpack())
            .map_err(|e| anyhow!("invalid sudt amount {}", e))
    };

    let custodian_lock = Script::new_builder()
        .code_hash(rollup_context.rollup_config.custodian_script_type_hash())
        .hash_type(ScriptHashType::Type.into())
        .args(rollup_context.rollup_script_hash.as_slice().pack())
        .build();
    let filter = min_capacity.map(|min_capacity| SearchKeyFilter {
        output_capacity_range: Some([min_capacity.into(), u64::MAX.into()]), // [inclusive, exclusive]
        ..Default::default()
    });

    let search_key = SearchKey::with_lock(custodian_lock).with_filter(filter);
    // order by ASC so we can search more cells
    let order = Order::Asc;

    let mut collected = CollectedCustodianCells::default();
    let mut collected_fullfilled_sudt = HashSet::new();
    let mut cursor = CollectLocalAndIndexerCursor::Local;

    // withdrawal ckb + change custodian capacity
    let required_capacity = {
        let withdrawal_capacity = withdrawals_amount.capacity;
        withdrawal_capacity.saturating_add(custodian_change_capacity)
    };

    while collected.capacity < required_capacity
        || collected_fullfilled_sudt.len() < withdrawals_amount.sudt.len()
    {
        let cells = collect_local_and_indexer_cells(
            local_cells_manager,
            indexer,
            &search_key,
            &order,
            None,
            &mut cursor,
        )
        .await?;

        if cursor.is_ended() {
            return Ok(QueryResult::NotEnough(collected));
        }

        query_indexer_times += 1;
        query_indexer_cells += cells.len();

        for cell in cells {
            if collected.cells_info.len() >= max_cells {
                return Ok(QueryResult::NotEnough(collected));
            }

            // Skip ckb custodians if capacity is fullfill
            if collected.capacity >= required_capacity
                && !withdrawals_amount.sudt.is_empty()
                && cell.output.type_().is_none()
            {
                continue;
            }

            let args = cell.output.as_reader().lock().args().raw_data();
            let custodian_lock_args = match CustodianLockArgsReader::from_slice(&args[32..]) {
                Ok(r) => r,
                Err(_) => continue,
            };

            if !compatible_finalized_timepoint.is_finalized(&Timepoint::from_full_value(
                custodian_lock_args.deposit_block_number().unpack(),
            )) {
                continue;
            }

            // Collect sudt
            if let Some(sudt_type_script) = cell.output.type_().to_opt() {
                // Invalid custodian type script
                let l1_sudt_script_type_hash =
                    rollup_context.rollup_config.l1_sudt_script_type_hash();
                if sudt_type_script.code_hash() != l1_sudt_script_type_hash
                    || sudt_type_script.hash_type() != ScriptHashType::Type.into()
                {
                    continue;
                }

                let sudt_type_hash = sudt_type_script.hash();
                if sudt_type_hash != CKB_SUDT_SCRIPT_ARGS {
                    // Already collected enough sudt amount
                    if collected_fullfilled_sudt.contains(&sudt_type_hash) {
                        continue;
                    }

                    // Not target withdrawal sudt
                    let withdrawal_amount = match withdrawals_amount.sudt.get(&sudt_type_hash) {
                        Some(amount) => amount,
                        None => continue,
                    };

                    let sudt_amount = match parse_sudt_amount(&cell) {
                        Ok(amount) => amount,
                        Err(_) => {
                            log::error!("invalid sudt amount, out_point: {:?}", cell.out_point);
                            continue;
                        }
                    };

                    let (collected_amount, type_script) = {
                        collected
                            .sudt
                            .entry(sudt_type_hash)
                            .or_insert((0, Script::default()))
                    };
                    *collected_amount = collected_amount.saturating_add(sudt_amount);
                    *type_script = sudt_type_script;

                    if *collected_amount >= *withdrawal_amount {
                        collected_fullfilled_sudt.insert(sudt_type_hash);
                    }
                }
            }

            collected.capacity = collected
                .capacity
                .saturating_add(cell.output.capacity().unpack().into());

            collected.cells_info.push(cell);

            if collected.cells_info.len() >= MAX_CELLS {
                if collected.capacity >= required_capacity {
                    break;
                } else {
                    log::debug!("[query finalized custodian cells] query indexer times: {} query indexer cells: {} duration: {}ms", query_indexer_times, query_indexer_cells, now.elapsed().as_millis());
                    return Ok(QueryResult::NotEnough(collected));
                }
            }
        }
    }

    log::debug!("[query finalized custodian cells] query indexer times: {} query indexer cells: {} duration: {}ms", query_indexer_times, query_indexer_cells, now.elapsed().as_millis());
    Ok(QueryResult::Full(collected))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use gw_rpc_client::indexer_client::CKBIndexerClient;
    use gw_rpc_client::rpc_client::QueryResult;
    use gw_types::bytes::Bytes;
    use gw_types::core::{ScriptHashType, Timepoint};
    use gw_types::offchain::{CellInfo, CompatibleFinalizedTimepoint, WithdrawalsAmount};
    use gw_types::packed::{
        CellOutput, CustodianLockArgs, OutPoint, RollupConfig, Script, Uint128,
    };
    use gw_types::prelude::{Builder, Entity, Pack, Unpack};
    use gw_utils::local_cells::LocalCellsManager;
    use gw_utils::RollupContext;

    const CKB: u64 = 100_000_000;

    #[tokio::test]
    async fn test_query_finalized_custodians() {
        let rollup_context = RollupContext {
            rollup_script_hash: [1u8; 32].into(),
            rollup_config: RollupConfig::new_builder()
                .custodian_script_type_hash([2u8; 32].pack())
                .l1_sudt_script_type_hash([3u8; 32].pack())
                .build(),
            fork_config: Default::default(),
        };

        let sudt_script = Script::new_builder()
            .code_hash([3u8; 32].pack())
            .hash_type(ScriptHashType::Type.into())
            .args(Bytes::from_static(b"33").pack())
            .build();

        let withdrawals_amount = WithdrawalsAmount {
            capacity: (1000 * CKB) as u128,
            sudt: HashMap::from([(sudt_script.hash(), 500u128); 1]),
        };

        let last_finalized_block_number = 100;
        let last_finalized_timepoint = Timepoint::from_block_number(last_finalized_block_number);
        let compatible_finalized_timepoint =
            CompatibleFinalizedTimepoint::from_block_number(last_finalized_block_number, 0);
        let ten_ckb_cells = generate_finalized_ckb_custodian_cells(
            10,
            &rollup_context,
            &last_finalized_timepoint,
            1000 * CKB,
        );
        let one_sudt_cell = generate_finalized_sudt_custodian_cells(
            1,
            &rollup_context,
            &last_finalized_timepoint,
            1000 * CKB,
            sudt_script.clone(),
            1000u128.pack(),
        );

        let max_five_cells = 5;
        let change_capacity = 0;

        let mut local_cells_manager = LocalCellsManager::default();
        for c in ten_ckb_cells.into_iter().chain(one_sudt_cell) {
            local_cells_manager.add_live(c);
        }

        let indexer_client = CKBIndexerClient::with_url("http://host.invalid").unwrap();

        let result = super::query_finalized_custodian_cells(
            &local_cells_manager,
            &indexer_client,
            &rollup_context,
            &withdrawals_amount,
            change_capacity,
            &compatible_finalized_timepoint,
            None,
            max_five_cells,
        )
        .await
        .unwrap();

        assert!(matches!(result, QueryResult::Full(_)));
    }

    fn generate_finalized_ckb_custodian_cells(
        cell_num: usize,
        rollup_context: &RollupContext,
        last_finalized_timepoint: &Timepoint,
        capacity: u64,
    ) -> Vec<CellInfo> {
        let args = {
            let custodian_lock_args = CustodianLockArgs::new_builder()
                .deposit_block_number(last_finalized_timepoint.full_value().pack())
                .build();

            let mut args = rollup_context.rollup_script_hash.as_slice().to_vec();
            args.extend_from_slice(custodian_lock_args.as_slice());

            Bytes::from(args)
        };
        let lock = Script::new_builder()
            .code_hash(rollup_context.rollup_config.custodian_script_type_hash())
            .hash_type(ScriptHashType::Type.into())
            .args(args.pack())
            .build();
        let output = CellOutput::new_builder()
            .capacity(capacity.pack())
            .lock(lock)
            .build();

        (0..cell_num)
            .map(|i| CellInfo {
                output: output.clone(),
                data: Default::default(),
                out_point: OutPoint::new_builder().index((i as u32).pack()).build(),
            })
            .collect()
    }

    fn generate_finalized_sudt_custodian_cells(
        cell_num: usize,
        rollup_context: &RollupContext,
        last_finalized_timepoint: &Timepoint,
        capacity: u64,
        sudt_script: Script,
        amount: Uint128,
    ) -> Vec<CellInfo> {
        let ckb_cells = generate_finalized_ckb_custodian_cells(
            cell_num,
            rollup_context,
            last_finalized_timepoint,
            capacity,
        );

        let convert_to_sudt = |cell: CellInfo| {
            let output = cell
                .output
                .as_builder()
                .type_(Some(sudt_script.clone()).pack())
                .build();
            let mut idx: u32 = cell.out_point.index().unpack();
            idx += 10000;
            CellInfo {
                output,
                data: amount.as_bytes(),
                out_point: cell.out_point.as_builder().index(idx.pack()).build(),
            }
        };
        ckb_cells.into_iter().map(convert_to_sudt).collect()
    }
}
