use std::{convert::TryInto, sync::Arc, time::Instant};

use anyhow::{anyhow, Result};
use ckb_fixed_hash::H256 as JsonH256;
use ckb_types::prelude::{Builder, Entity};
use gw_generator::Generator;
use gw_jsonrpc_types::{ckb_jsonrpc_types::Uint64, debug::DebugRunResult};
use gw_store::{
    chain_view::ChainView,
    state::{
        history::history_state::{RWConfig, ReadOpt, WriteOpt},
        overlay::mem_store::MemStore,
        traits::JournalDB,
        BlockStateDB,
    },
    traits::chain_store::ChainStore,
    Store,
};
use gw_types::{packed::BlockInfo, prelude::Unpack};
use jsonrpc_v2::{Data, Params};

use crate::utils::to_h256;

pub(crate) struct DebugTransactionContext {
    pub store: Store,
    pub generator: Arc<Generator>,
    pub debug_generator: Arc<Generator>,
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub(crate) enum DebugReplayTxParams {
    Default((JsonH256,)),
    WithMaxCycles((JsonH256, Uint64)),
}

pub(crate) async fn replay_transaction(
    Params(param): Params<DebugReplayTxParams>,
    ctx: Data<DebugTransactionContext>,
) -> Result<Option<DebugRunResult>> {
    let (tx_hash, max_cycles) = match param {
        DebugReplayTxParams::Default((tx_hash,)) => (tx_hash, None),
        DebugReplayTxParams::WithMaxCycles((tx_hash, cycles)) => (tx_hash, Some(cycles.value())),
    };
    let tx_hash = to_h256(tx_hash);

    if ctx.store.get_transaction(&tx_hash)?.is_none() {
        return Ok(None);
    }

    // run target tx
    let run_result: DebugRunResult = tokio::task::spawn_blocking(move || {
        let db = &ctx.store.begin_transaction();

        // find tx info
        let info = db
            .get_transaction_info(&tx_hash)?
            .ok_or_else(|| anyhow!("can't find tx on the chain"))?;
        let block_number = info.block_number().unpack();
        let tx_index = info.key().index();
        let block = db
            .get_block(&info.key().block_hash())?
            .ok_or_else(|| anyhow!("can't find block"))?;

        // build history state
        let mem_db = MemStore::new(db);
        let parent_block_number = block_number.saturating_sub(1u64);
        let mut hist_state = BlockStateDB::from_store(
            mem_db,
            RWConfig {
                read: ReadOpt::Block(parent_block_number),
                write: WriteOpt::Block(parent_block_number),
            },
        )?;
        let tip_block_hash = db.get_last_valid_tip_block_hash()?;
        let chain_view = ChainView::new(&db, tip_block_hash);
        let block_info = {
            let raw = block.raw();
            BlockInfo::new_builder()
                .block_producer(raw.block_producer())
                .timestamp(raw.timestamp())
                .number(raw.number())
                .build()
        };
        // execute prev txs
        for i in 0..tx_index {
            let tx = block.transactions().get(i as usize).unwrap();
            let raw_tx = tx.raw();
            ctx.generator.execute_transaction(
                &chain_view,
                &mut hist_state,
                &block_info,
                &raw_tx,
                None,
                None,
            )?;
            hist_state.finalise()?;
        }

        // execute target with debug generator
        let tx = block.transactions().get(tx_index as usize).unwrap();
        let raw_tx = tx.raw();
        let t = Instant::now();
        let run_result = ctx.debug_generator.execute_transaction(
            &chain_view,
            &mut hist_state,
            &block_info,
            &raw_tx,
            max_cycles,
            None,
        )?;
        let execution_time = t.elapsed();

        // finalise
        let t = Instant::now();
        hist_state.finalise()?;
        let write_mem_smt_time = t.elapsed();

        // record time
        let mut debug_run_result: DebugRunResult = run_result.try_into()?;
        debug_run_result.execution_time_ms = execution_time.as_millis().try_into()?;
        debug_run_result.write_mem_smt_time_ms = write_mem_smt_time.as_millis().try_into()?;

        Result::<_, anyhow::Error>::Ok(debug_run_result)
    })
    .await??;

    // generate response
    Ok(Some(run_result))
}
