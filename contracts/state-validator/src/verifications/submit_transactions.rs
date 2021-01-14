use crate::types::BlockContext;
use crate::error::Error;
use gw_common::{blake2b::new_blake2b, state::State, merkle_utils::calculate_merkle_root};
use gw_types::{packed::L2Block, prelude::*};

/// Handle SubmitTransactions
pub fn verify(context: &mut Context, block: &L2Block) -> Result<(), Error> {
    // Verify tx_witness_root

    let submit_transactions = match block.raw().submit_transactions().to_opt() {
        Some(submit_transactions) => submit_transactions,
        None => return Ok(()),
    };
    let tx_witness_root = submit_transactions.tx_witness_root().unpack();
    let tx_count: u32 = submit_transactions.tx_count().unpack();
    let compacted_post_root_list = submit_transactions.compacted_post_root_list();

    if tx_count != compacted_post_root_list.item_count() as u32 {
        return Err(Error::InvalidTxs);
    }

    let leaves = block
        .transactions()
        .into_iter()
        .map(|tx| {
            tx.hash()
        })
        .collect();
    let merkle_root: [u8; 32] = calculate_merkle_root(leaves)?;
    if tx_witness_root != merkle_root {
        return Err(Error::InvalidTxs);
    }

    Ok(())
}
