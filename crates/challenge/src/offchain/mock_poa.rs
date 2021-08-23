use anyhow::Result;
use gw_poa::{PoA, PoAContext};
use gw_rpc_client::RPCClient;
use gw_types::bytes::Bytes;
use gw_types::offchain::{CellInfo, InputCellInfo};
use gw_types::packed::{CellDep, CellOutput, Script};
use gw_types::prelude::*;

use std::time::Duration;

pub struct MockPoA {
    pub cell_deps: Vec<CellDep>,
    pub input_since: u64,
    pub inputs: Vec<InputCellInfo>,
    pub outputs: Vec<(CellOutput, Bytes)>,
    pub lock: Script,
}

impl MockPoA {
    pub async fn build(
        rpc_client: &RPCClient,
        poa: &PoA,
        rollup_cell: &InputCellInfo,
    ) -> Result<Self> {
        let median_time = {
            let l1_tip_block_hash = rpc_client.get_tip().await?.block_hash().unpack();
            rpc_client.get_block_median_time(l1_tip_block_hash).await?
        };
        let context = poa.query_poa_context(rollup_cell).await?;

        let poa_context = MockPoA::ensure_unlockable(context, poa, median_time);
        let generated_poa = poa
            .generate_by_context(poa_context, &vec![], median_time)
            .await?;

        let mock_poa = MockPoA {
            cell_deps: generated_poa.cell_deps,
            input_since: generated_poa.poa_input_cell_since,
            inputs: generated_poa.input_cells,
            outputs: generated_poa.output_cells,
            lock: rollup_cell.cell.output.lock(),
        };

        Ok(mock_poa)
    }

    fn ensure_unlockable(mut context: PoAContext, poa: &PoA, median_time: Duration) -> PoAContext {
        let next_round_start_time = poa.estimate_next_round_start_time(context.clone());
        // Already unlocked
        if median_time >= next_round_start_time {
            return context;
        }

        let unlocked_round_initial_subtime = {
            let diff = next_round_start_time.as_secs() - median_time.as_secs();
            context.poa_data.round_initial_subtime().unpack() - diff - 1
        };
        let unlocked_data = {
            let builder = context.poa_data.as_builder();
            builder
                .round_initial_subtime(unlocked_round_initial_subtime.pack())
                .build()
        };
        let unlocked_cell = CellInfo {
            out_point: context.poa_data_cell.out_point,
            output: context.poa_data_cell.output,
            data: unlocked_data.as_bytes(),
        };

        context.poa_data = unlocked_data;
        context.poa_data_cell = unlocked_cell;

        context
    }
}
