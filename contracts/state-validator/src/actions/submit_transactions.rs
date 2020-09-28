use crate::blake2b::new_blake2b;
use crate::context::Context;
use crate::error::Error;
use crate::util::{calculate_compacted_account_root, calculate_merkle_root};
use godwoken_types::{packed::L2Block, prelude::*};

/// Handle SubmitTransactions
pub fn handle(context: &mut Context, block: &L2Block) -> Result<(), Error> {
    // Verify tx_root

    let submit_transactions = match block.raw().submit_transactions().to_opt() {
        Some(submit_transactions) => submit_transactions,
        None => return Ok(()),
    };
    let tx_root = submit_transactions.tx_root().unpack();
    let tx_count: u32 = submit_transactions.tx_count().unpack();
    let compacted_pre_root_list = submit_transactions.compacted_pre_root_list();

    if tx_count != compacted_pre_root_list.item_count() as u32 {
        return Err(Error::InvalidTxs);
    }

    let first_compacted_root =
        calculate_compacted_account_root(&context.calculate_account_root()?, context.account_count);
    if compacted_pre_root_list.get(0).map(|root| root.unpack()) != Some(first_compacted_root) {
        return Err(Error::InvalidTxs);
    }

    let leaves = block
        .transactions()
        .into_iter()
        .map(|tx| {
            let mut buf = [0u8; 32];
            let mut hasher = new_blake2b();
            hasher.update(tx.as_slice());
            hasher.finalize(&mut buf);
            buf
        })
        .collect();
    let merkle_root: [u8; 32] = calculate_merkle_root(leaves)?;
    if tx_root != merkle_root {
        return Err(Error::InvalidTxs);
    }

    Ok(())
}
