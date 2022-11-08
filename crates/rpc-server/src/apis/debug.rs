use anyhow::Result;
use ckb_fixed_hash::H256 as JsonH256;
use gw_store::{
    state::{
        history::history_state::{RWConfig, ReadOpt, WriteOpt},
        overlay::mem_store::MemStore,
        BlockStateDB,
    },
    Store, traits::chain_store::ChainStore,
};
use gw_types::prelude::Unpack;
use jsonrpc_v2::Data;

use crate::utils::to_h256;

async fn replay_transaction(tx_hash: JsonH256, store: Data<Store>) -> Result<()> {
    let tx_hash = to_h256(tx_hash);
    let db = &store.begin_transaction();
    match db.get_transaction_info(&tx_hash)? {
        Some(info) =>{
            let block_number = info.block_number().unpack();
        }
        None => {}
    }
    let mem_db = MemStore::new(db);
    let block_number = 5;
    let hist_state = BlockStateDB::from_store(
        mem_db,
        RWConfig {
            read: ReadOpt::Block(block_number),
            write: WriteOpt::Block(block_number),
        },
    )?;
    Ok(())
}
