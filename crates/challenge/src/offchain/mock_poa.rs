use anyhow::Result;
use gw_config::BlockProducerConfig;
use gw_poa::{PoA, PoAContext};
use gw_rpc_client::RPCClient;
use gw_types::bytes::Bytes;
use gw_types::offchain::{CellInfo, InputCellInfo};
use gw_types::packed::PoAData;
use gw_types::packed::{CellDep, CellInput, CellOutput};
use gw_types::prelude::*;

use std::time::Duration;

pub struct MockPoA {
    pub lock_dep: CellDep,
    pub state_dep: CellDep,
    pub setup_dep: InputCellInfo,
    pub data_input: InputCellInfo,
    pub output: (CellOutput, Bytes),
}

impl MockPoA {
    pub async fn build(
        rpc_client: &RPCClient,
        poa: &PoA,
        rollup_cell: &InputCellInfo,
        config: &BlockProducerConfig,
    ) -> Result<Self> {
        let context = poa.query_poa_context(rollup_cell).await?;
        let median_time = {
            let l1_tip_block_hash = rpc_client.get_tip().await?.block_hash().unpack();
            rpc_client.get_block_median_time(l1_tip_block_hash).await?
        };

        let input_poa_data = MockPoA::ensure_unlockable(&context, median_time);
        let output_poa_data = MockPoA::data_output(&input_poa_data, &context, median_time);
        let data_input = {
            let mut cell = context.poa_data_cell.clone();
            cell.data = input_poa_data.as_bytes();
            into_input_cell_info(cell)
        };
        let output = (
            context.poa_data_cell.output.clone(),
            output_poa_data.as_bytes(),
        );
        let setup_dep = into_input_cell_info(context.poa_setup_cell);

        Ok(MockPoA {
            lock_dep: config.poa_lock_dep.clone().into(),
            state_dep: config.poa_state_dep.clone().into(),
            setup_dep,
            data_input,
            output,
        })
    }

    fn ensure_unlockable(ctx: &PoAContext, median_time: Duration) -> PoAData {
        let identities_len = ctx.poa_setup.identities.len() as u64;
        let mut steps = (ctx.block_producer_index as u64)
            .saturating_add(identities_len)
            .saturating_sub(Unpack::<u16>::unpack(&ctx.poa_data.block_producer_index()) as u64)
            % identities_len;
        if steps == 0 {
            steps = identities_len;
        }

        let poa_data = ctx.poa_data.clone();
        let initial_time: u64 = poa_data.round_initial_subtime().unpack();
        let next_start_time = initial_time + ctx.poa_setup.round_intervals as u64 * steps;
        if next_start_time <= median_time.as_secs() {
            return poa_data;
        }

        let round_initial_subtime = next_start_time.saturating_sub(median_time.as_secs() + 1);
        let poa_data = {
            let builder = poa_data.as_builder();
            builder.round_initial_subtime(round_initial_subtime.pack())
        };

        poa_data.build()
    }

    fn data_output(poa_data: &PoAData, ctx: &PoAContext, median_time: Duration) -> PoAData {
        let data_round_initial_subtime: u64 = poa_data.round_initial_subtime().unpack();
        let data_subblock_index: u32 = poa_data.subblock_index().unpack();
        let data_subblock_subtime: u64 = poa_data.subblock_subtime().unpack();
        let data_block_producer_index = poa_data.block_producer_index();
        if median_time.as_secs() < data_round_initial_subtime + ctx.poa_setup.round_intervals as u64
            && data_subblock_index + 1 < ctx.poa_setup.subblocks_per_round
        {
            PoAData::new_builder()
                .round_initial_subtime(data_round_initial_subtime.pack())
                .subblock_subtime((data_subblock_subtime + 1).pack())
                .subblock_index((data_subblock_index + 1).pack())
                .block_producer_index(data_block_producer_index)
                .build()
        } else {
            PoAData::new_builder()
                .round_initial_subtime(median_time.as_secs().pack())
                .subblock_subtime(median_time.as_secs().pack())
                .subblock_index(0u32.pack())
                .block_producer_index(ctx.block_producer_index.pack())
                .build()
        }
    }
}

fn into_input_cell_info(cell_info: CellInfo) -> InputCellInfo {
    InputCellInfo {
        input: CellInput::new_builder()
            .previous_output(cell_info.out_point.clone())
            .build(),
        cell: cell_info,
    }
}
