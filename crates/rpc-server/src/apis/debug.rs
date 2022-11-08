use std::convert::TryInto;

use anyhow::{anyhow, Result};
use ckb_fixed_hash::H256 as JsonH256;
use ckb_types::prelude::{Builder, Entity};
use gw_generator::{constants::L2TX_MAX_CYCLES, Generator};
use gw_jsonrpc_types::debug::DebugRunResult;
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

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub(crate) enum DebugReplayTxParams {
    Default((JsonH256,)),
}

pub(crate) async fn replay_transaction(
    Params(param): Params<DebugReplayTxParams>,
    store: Data<Store>,
    generator: Data<Generator>,
) -> Result<Option<DebugRunResult>> {
    let DebugReplayTxParams::Default((tx_hash,)) = param;
    let tx_hash = to_h256(tx_hash);

    if store.get_transaction(&tx_hash)?.is_none() {
        return Ok(None);
    }

    // run target tx
    let run_result = tokio::task::spawn_blocking(move || {
        let db = &store.begin_transaction();

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
        let mut hist_state = BlockStateDB::from_store(
            mem_db,
            RWConfig {
                read: ReadOpt::Block(block_number),
                write: WriteOpt::Block(block_number),
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
            generator.unchecked_execute_transaction(
                &chain_view,
                &mut hist_state,
                &block_info,
                &raw_tx,
                L2TX_MAX_CYCLES,
                None,
            )?;
            hist_state.finalise()?;
        }

        // execute target
        let tx = block.transactions().get(tx_index as usize).unwrap();
        let raw_tx = tx.raw();
        let run_result = generator.unchecked_execute_transaction(
            &chain_view,
            &mut hist_state,
            &block_info,
            &raw_tx,
            L2TX_MAX_CYCLES,
            None,
        )?;
        hist_state.finalise()?;

        Result::<_, anyhow::Error>::Ok(run_result)
    })
    .await??;

    // generate response
    Ok(Some(run_result.try_into()?))
}
