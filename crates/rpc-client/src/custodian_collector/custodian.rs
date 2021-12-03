use std::{
    cmp::{Ordering, Reverse},
    collections::{BinaryHeap, HashMap},
};

use crate::{
    indexer_types::{Cell, Pagination},
    rpc_client::QueryResult,
};
use anyhow::{anyhow, Result};
use gw_types::{
    core::ScriptHashType,
    offchain::{CellInfo, CollectedCustodianCells, WithdrawalsAmount},
    packed::{CellOutput, CustodianLockArgs, CustodianLockArgsReader, OutPoint, Script},
    prelude::*,
};

use super::traits::Collector;

// This function try to fulfilled custodians within given capped cell limit.
// It will cache up to `max_custodian_cells` different type of sudt custodians(include
// withdrawals). Then fill ckb and withdrawal sudt. If there are rooms for custodians
// to combine, it will fill ckb, withdrawal sudts, then remain cached sudts.
pub async fn query_finalized_custodian_capped_cells(
    collector: impl Collector,
    withdrawals_amount: &WithdrawalsAmount,
    custodian_change_capacity: u128,
    last_finalized_block_number: u64,
    max_custodian_cells: usize,
) -> Result<QueryResult<CollectedCustodianCells>> {
    const MAX_CACHE_SUDT_TYPES: usize = 500;

    log::info!(
        "[collect custodian cell] start max_custodian_cells: {}, max_sudt_types: {}",
        max_custodian_cells,
        MAX_CACHE_SUDT_TYPES,
    );

    let parse_sudt_amount = |cell: &Cell| -> Result<u128> {
        if cell.output.type_.is_none() {
            return Err(anyhow!("no a sudt cell"));
        }

        gw_types::packed::Uint128::from_slice(cell.output_data.as_bytes())
            .map(|a| a.unpack())
            .map_err(|e| anyhow!("invalid sudt amount {}", e))
    };

    let search_key = collector.build_search_key();

    let mut ckb_candidates = CandidateCustodians::default();
    let mut sudt_candidates: HashMap<[u8; 32], CandidateCustodians<_>> = HashMap::new();
    let mut candidate_cells = 0usize;
    let mut candidate_capacity = 0u128;
    let mut cursor = None;

    // withdrawal ckb + change custodian capacity
    let required_capacity = {
        let withdrawal_capacity = withdrawals_amount.capacity;
        withdrawal_capacity.saturating_add(custodian_change_capacity)
    };

    while candidate_capacity < required_capacity
        || sudt_candidates.values().filter(|c| c.fulfilled).count() < withdrawals_amount.sudt.len()
        || sudt_candidates
            .values()
            .filter(|c| c.cells.len() > 5)
            .map(|c| c.cells.len())
            .sum::<usize>()
            < max_custodian_cells
    {
        let cells: Pagination<Cell> = collector.get_cells(&search_key, cursor)?;
        log::info!(
            "collect custodian cells from indexer {}, candidate cell {} max_cell {}",
            cells.objects.len(),
            candidate_cells,
            max_custodian_cells
        );

        if cells.last_cursor.is_empty() {
            break;
        }
        cursor = Some(cells.last_cursor);

        for cell in cells.objects.into_iter() {
            let args = cell.output.lock.args.clone().into_bytes();
            let custodian_lock_args = match CustodianLockArgsReader::verify(&args[32..], false) {
                Ok(()) => CustodianLockArgs::new_unchecked(args.slice(32..)),
                Err(_) => {
                    log::info!("cell fail to parse args");
                    continue;
                }
            };

            if custodian_lock_args.deposit_block_number().unpack() > last_finalized_block_number {
                log::info!("cell fail to check finalize time");
                continue;
            }

            let opt_sudt_type_script = cell.output.type_.clone().map(|json_script| {
                let script = ckb_types::packed::Script::from(json_script);
                Script::new_unchecked(script.as_bytes())
            });

            // Verify sudt
            if let Some(sudt_type_script) = opt_sudt_type_script.as_ref() {
                // Invalid custodian type script
                let l1_sudt_script_type_hash = collector.l1_sudt_script_type_hash();
                if sudt_type_script.code_hash() != l1_sudt_script_type_hash
                    || sudt_type_script.hash_type() != ScriptHashType::Type.into()
                {
                    log::info!("fail to check l1 sudt");
                    continue;
                }
            }

            let (sudt_amount, type_hash) = match opt_sudt_type_script.as_ref() {
                Some(h) => match parse_sudt_amount(&cell) {
                    Ok(amount) => (amount, CustodianTypeHash::Sudt(h.hash())),
                    Err(_) => {
                        log::error!("invalid sudt amount, out_point: {:?}", cell.out_point);
                        continue;
                    }
                },
                _ => (0, CustodianTypeHash::Ckb),
            };

            // Only cache up to `max_custodian_cells` different type of sudts(include withdrawal
            // sudts)
            if type_hash.is_sudt()
                && !withdrawals_amount.sudt.contains_key(&type_hash.raw())
                && sudt_candidates.len() > MAX_CACHE_SUDT_TYPES
            {
                log::info!("fail to check sudt candidates");
                continue;
            }

            let info = to_cell_info(cell);
            let custodian_cell = CustodianCell {
                capacity: info.output.capacity().unpack(),
                amount: sudt_amount,
                info,
            };

            // Store custodian cell in binary heap, sort by amount and capacity in reverse
            // ordering. Minimal amount/capacity custodian cell get popped first.
            let custodians = match type_hash {
                CustodianTypeHash::Ckb => &mut ckb_candidates,
                CustodianTypeHash::Sudt(raw_hash) => sudt_candidates
                    .entry(raw_hash)
                    .or_insert_with(CandidateCustodians::<Reverse<_>>::default),
            };

            let custodian_capacity = custodian_cell.capacity as u128;
            if let Err(AmountOverflow) = custodians.push(type_hash, Reverse(custodian_cell)) {
                log::info!("sudt amount overflow");
                continue;
            }
            candidate_cells += 1;
            candidate_capacity = candidate_capacity.saturating_add(custodian_capacity);

            // For every custodian, we only cache to `max_custodian_cells` cells.
            // Replace minimal amount/capacity with bigger one.
            if custodians.cells.len() > max_custodian_cells {
                let min = custodians.pop().expect("minimal custodian");
                candidate_cells -= 1;
                candidate_capacity = candidate_capacity.saturating_sub(min.capacity as u128);
            }

            // Skip sudt fulfilled check if already fulfilled
            if custodians.fulfilled || custodians.type_hash.is_ckb() {
                log::info!(
                    "fulfilled {}, is_ckb {}",
                    custodians.fulfilled,
                    custodians.type_hash.is_ckb()
                );
                continue;
            }

            // Check whether collected sudt amount fulfill withdrawal requests
            if let Some(withdrawal_amount) = withdrawals_amount.sudt.get(&type_hash.raw()) {
                // Mark withdrawal custodians
                custodians.withdrawal = true;
                if custodians.amount >= *withdrawal_amount {
                    custodians.fulfilled = true;
                }
            }
        }
    }

    log::info!("phase 1, candidate cells: {}", candidate_cells);

    // No withdrawals, check whether we have custodians to defragment
    if withdrawals_amount.capacity == 0 {
        sudt_candidates = sudt_candidates
            .into_iter()
            .filter(|(_, candidates)| candidates.cells.len() > 5)
            .collect();
        if sudt_candidates.is_empty() || ckb_candidates.cells.len() <= 5 {
            log::info!(
                "sudt candidates {} ckb candidates cells: {}",
                sudt_candidates.len(),
                ckb_candidates.cells.len()
            );
            return Ok(QueryResult::NotEnough(CollectedCustodianCells::default()));
        }
    }

    // Reverse ckb binary heap, sort by capacity, bigger one comes first.
    let mut ckb_candidates = ckb_candidates.reverse();
    let mut sudt_candidates: Vec<CandidateCustodians<_>> = {
        // Reverse sudt binary heap, sort by fulfilled, withdrawal, capacity, amount, cell_len.
        let binary_heap: BinaryHeap<CandidateCustodians<_>> = sudt_candidates
            .into_iter()
            .map(|(_, reverse_custodians)| reverse_custodians.reverse())
            .collect();
        // Collect sorted sudt candidate custodians into vec, so we can iter_mut()
        binary_heap.into_sorted_vec()
    };

    let mut collected = CollectedCustodianCells::default();
    let mut fulfilled_sudt = 0usize;

    // Fill ckb custodians first since we need capacity everywhere
    log::info!("ckb remain {}", ckb_candidates.cells.len());
    while ckb_candidates.cells.peek().is_some() {
        if collected.cells_info.len() > max_custodian_cells
            || collected.capacity >= required_capacity
        {
            log::info!(
                    "collected.cells_info {} max_custodian_cells {}, collected.capacity {} required_capacity: {}",
                    collected.cells_info.len(),
                    max_custodian_cells,
                    collected.capacity,
                    required_capacity,
                );
            break;
        }

        if let Some(cell) = ckb_candidates.cells.pop() {
            collected.capacity = collected.capacity.saturating_add(cell.capacity as u128);
            collected.cells_info.push(cell.info);
        }
    }

    // Fill sudt custodians for withdrawal requests
    log::info!("sudt_candidates {}", sudt_candidates.len(),);
    'fill_for_withdrawals: for custodians in sudt_candidates.iter_mut() {
        log::info!("sudt_remains {}", custodians.cells.len(),);
        while custodians.cells.peek().is_some() {
            if collected.cells_info.len() > max_custodian_cells
                || (collected.capacity >= required_capacity
                    && fulfilled_sudt == withdrawals_amount.sudt.len())
            {
                log::info!("collected cells_info.len() {} max_custodian_cells {}, collected.capacity {} required_cpacity: {} fulfilled_sudt {} withdrawals_amount.sudt.len(): {}",
                    collected.cells_info.len(),
                    max_custodian_cells,
                    collected.capacity,
                    required_capacity,
                    fulfilled_sudt,
                    withdrawals_amount.sudt.len()

                    );
                break 'fill_for_withdrawals;
            }

            if let Some(cell) = custodians.cells.pop() {
                collected.capacity = collected.capacity.saturating_add(cell.capacity as u128);
                collected.cells_info.push(cell.info.clone());

                let (collected_amount, _) = {
                    let sudt = collected.sudt.entry(custodians.type_hash.raw());
                    sudt.or_insert((0, cell.info.output.type_().to_opt().unwrap_or_default()))
                };
                *collected_amount = collected_amount
                    .checked_add(cell.amount)
                    .expect("already check overflow");

                let withdrawal_amount = withdrawals_amount.sudt.get(&custodians.type_hash.raw());
                if Some(&*collected_amount) >= withdrawal_amount {
                    log::info!(
                        "collected_amount: {} withdrawal_amount: {:?}",
                        collected_amount,
                        withdrawal_amount
                    );
                    fulfilled_sudt += 1;
                    break;
                }
            }
        }
    }

    // Now if we still have room then fill remain custodians for defragment
    // Ckb first
    while let Some(cell) = ckb_candidates.cells.pop() {
        if collected.cells_info.len() > max_custodian_cells {
            log::info!(
                "collected.cells_info.len(): {} max_custodian_cells: {}",
                collected.cells_info.len(),
                max_custodian_cells
            );
            break;
        }

        collected.capacity = collected.capacity.saturating_add(cell.capacity as u128);
        collected.cells_info.push(cell.info);
    }

    'fill_for_merge: for mut custodians in sudt_candidates {
        let sudt_remains = &mut custodians.cells;
        let collected_cells = collected.cells_info.len();

        // Withdrawal sudt custodians should be filled through `fill_for_withdrawals`, so
        // defragment them first.
        if !custodians.withdrawal
            && (sudt_remains.len() < 3 // Few custodians to combine
                    || (max_custodian_cells.saturating_sub(collected_cells) == 1))
        {
            log::info!(
                "sudt_remains.len(): {} max_custodian_cells: {} collected_cells: {}",
                sudt_remains.len(),
                max_custodian_cells,
                collected_cells
            );
            continue;
        }

        while let Some(cell) = sudt_remains.pop() {
            if collected.cells_info.len() > max_custodian_cells {
                log::info!(
                    "collected.cells_info.len(): {} max_custodian_cells: {}",
                    collected.cells_info.len(),
                    max_custodian_cells,
                );
                break 'fill_for_merge;
            }

            collected.capacity = collected.capacity.saturating_add(cell.capacity as u128);
            collected.cells_info.push(cell.info.clone());

            let (collected_amount, _) = {
                let sudt = collected.sudt.entry(custodians.type_hash.raw());
                let script = cell.info.output.type_().to_opt().unwrap_or_default();
                sudt.or_insert((0, script))
            };
            *collected_amount = collected_amount
                .checked_add(cell.amount)
                .expect("already check overflow");
        }
    }

    if fulfilled_sudt == withdrawals_amount.sudt.len() && collected.capacity >= required_capacity {
        log::info!(
            "[collect custodian cell] collect enough cells, capacity: {}, cells: {}",
            collected.capacity,
            collected.cells_info.len()
        );
        Ok(QueryResult::Full(collected))
    } else {
        log::info!(
            "[collect custodian cell] collect not enough cells, capacity: {}, cells: {}",
            collected.capacity,
            collected.cells_info.len()
        );
        Ok(QueryResult::NotEnough(collected))
    }
}

#[derive(Debug, PartialEq, Eq, Hash, Clone, Copy)]
enum CustodianTypeHash {
    Ckb,
    Sudt([u8; 32]),
}

impl CustodianTypeHash {
    fn is_ckb(&self) -> bool {
        matches!(self, CustodianTypeHash::Ckb)
    }

    fn is_sudt(&self) -> bool {
        matches!(self, CustodianTypeHash::Sudt(_))
    }

    fn raw(&self) -> [u8; 32] {
        match self {
            CustodianTypeHash::Ckb => unreachable!("no hash for ckb"),
            CustodianTypeHash::Sudt(hash) => *hash,
        }
    }
}

struct CustodianCell {
    capacity: u64,
    amount: u128,
    info: CellInfo,
}

// Sort by amount, then capacity
impl Ord for CustodianCell {
    fn cmp(&self, other: &Self) -> Ordering {
        let ord = self.amount.cmp(&other.amount);
        if matches!(ord, std::cmp::Ordering::Equal) {
            self.capacity.cmp(&other.capacity)
        } else {
            ord
        }
    }
}

impl PartialOrd for CustodianCell {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for CustodianCell {
    fn eq(&self, other: &Self) -> bool {
        self.amount == other.amount && self.capacity == other.capacity
    }
}

impl Eq for CustodianCell {}

struct AmountOverflow;

struct CandidateCustodians<T: Ord> {
    fulfilled: bool,
    withdrawal: bool,
    capacity: u128,
    amount: u128,
    type_hash: CustodianTypeHash,
    cell_len: usize,
    cells: BinaryHeap<T>,
}

impl CandidateCustodians<Reverse<CustodianCell>> {
    fn push(
        &mut self,
        type_hash: CustodianTypeHash,
        reverse_cell: Reverse<CustodianCell>,
    ) -> Result<(), AmountOverflow> {
        self.amount = {
            let amount = reverse_cell.0.amount;
            self.amount.checked_add(amount).ok_or(AmountOverflow)?
        };
        self.capacity = {
            let capacity = reverse_cell.0.capacity as u128;
            self.capacity.saturating_add(capacity)
        };

        self.type_hash = type_hash;
        self.cells.push(reverse_cell);
        self.cell_len = self.cells.len();

        Ok(())
    }

    fn pop(&mut self) -> Option<CustodianCell> {
        self.cells.pop().map(|reverse_cell| {
            let cell = reverse_cell.0;
            self.capacity = self.capacity.saturating_sub(cell.capacity as u128);
            self.amount = self.amount.saturating_sub(cell.amount);
            self.cell_len -= 1;
            cell
        })
    }

    fn reverse(mut self) -> CandidateCustodians<CustodianCell> {
        let cells = self.cells.drain().map(|r| r.0).collect();
        CandidateCustodians {
            fulfilled: self.fulfilled,
            withdrawal: self.withdrawal,
            capacity: self.capacity,
            amount: self.amount,
            type_hash: self.type_hash,
            cell_len: self.cell_len,
            cells,
        }
    }
}

impl<T: Ord> Default for CandidateCustodians<T> {
    fn default() -> Self {
        Self {
            fulfilled: false,
            withdrawal: false,
            capacity: 0,
            amount: 0,
            cell_len: 0,
            type_hash: CustodianTypeHash::Ckb,
            cells: BinaryHeap::new(),
        }
    }
}

impl<T: Ord> Ord for CandidateCustodians<T> {
    fn cmp(&self, other: &Self) -> Ordering {
        let mut ordering = (self.fulfilled as u8).cmp(&(other.fulfilled as u8));
        if !matches!(ordering, Ordering::Equal) {
            return ordering;
        }

        ordering = (self.withdrawal as u8).cmp(&(other.withdrawal as u8));
        if !matches!(ordering, Ordering::Equal) {
            return ordering;
        }

        ordering = self.capacity.cmp(&other.capacity);
        if !matches!(ordering, Ordering::Equal) {
            return ordering;
        }

        ordering = self.amount.cmp(&other.amount);
        if !matches!(ordering, Ordering::Equal) {
            return ordering;
        }

        self.cell_len.cmp(&other.cell_len)
    }
}

impl<T: Ord> PartialOrd for CandidateCustodians<T> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<T: Ord> PartialEq for CandidateCustodians<T> {
    fn eq(&self, other: &Self) -> bool {
        self.fulfilled == other.fulfilled
            && self.withdrawal == other.withdrawal
            && self.amount == other.amount
            && self.capacity == other.capacity
            && self.cell_len == other.cell_len
    }
}

impl<T: Ord> Eq for CandidateCustodians<T> {}

fn to_cell_info(cell: Cell) -> CellInfo {
    let out_point = {
        let out_point: ckb_types::packed::OutPoint = cell.out_point.into();
        OutPoint::new_unchecked(out_point.as_bytes())
    };
    let output = {
        let output: ckb_types::packed::CellOutput = cell.output.into();
        CellOutput::new_unchecked(output.as_bytes())
    };
    let data = cell.output_data.into_bytes();

    CellInfo {
        out_point,
        output,
        data,
    }
}
