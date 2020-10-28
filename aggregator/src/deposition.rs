use crate::collector::Collector;
use anyhow::{anyhow, Result};
use ckb_types::{
    bytes::Bytes,
    core::ScriptHashType,
    packed::{CellOutput, RawTransaction, Script},
    prelude::*,
};
use gw_common::{CKB_TOKEN_ID, DEPOSITION_CODE_HASH, SUDT_CODE_HASH};
use gw_generator::generator::DepositionRequest;
use gw_types::{
    packed::{DepositionLockArgs, DepositionLockArgsReader},
    prelude::Unpack as GWUnpack,
};

/// Fetch deposition requests of a tx
pub fn fetch_deposition_requests<C: Collector>(
    collector: &C,
    tx: &RawTransaction,
    rollup_id: &[u8; 32],
) -> Result<Vec<DepositionRequest>> {
    let mut deposition_requests = Vec::with_capacity(tx.inputs().len());
    // find deposition requests
    for (i, cell_input) in tx.inputs().into_iter().enumerate() {
        let previous_tx =
            collector.get_transaction(&cell_input.previous_output().tx_hash().unpack())?;
        let cell = previous_tx.transaction.raw().outputs().get(i).expect("get");
        let cell_data: Bytes = previous_tx
            .transaction
            .raw()
            .outputs_data()
            .get(i)
            .map(|data| data.unpack())
            .unwrap_or_default();
        let lock = cell.lock();
        let lock_code_hash: [u8; 32] = lock.code_hash().unpack();
        // not a deposition request lock
        if !(lock.hash_type() == ScriptHashType::Data.into()
            && lock_code_hash == DEPOSITION_CODE_HASH)
        {
            continue;
        }
        let args: Bytes = lock.args().unpack();
        let deposition_args = match DepositionLockArgsReader::verify(&args, false) {
            Ok(_) => DepositionLockArgs::new_unchecked(args),
            Err(_) => {
                return Err(anyhow!("invalid deposition request"))?;
            }
        };

        // ignore deposition request that do not belong to Rollup
        if &deposition_args.rollup_type_id().unpack() != rollup_id {
            continue;
        }

        // get token_id
        let token_id = fetch_token_id(cell.type_().to_opt())?;
        let value = fetch_sudt_value(&token_id, &cell, &cell_data);
        let deposition_request = DepositionRequest {
            token_id,
            value,
            pubkey_hash: deposition_args.pubkey_hash().unpack(),
            account_id: deposition_args.account_id().unpack(),
        };
        deposition_requests.push(deposition_request);
    }
    Ok(deposition_requests)
}

fn fetch_token_id(type_: Option<Script>) -> Result<[u8; 32]> {
    match type_ {
        Some(type_) => {
            let code_hash: [u8; 32] = type_.code_hash().unpack();
            if type_.hash_type() == ScriptHashType::Data.into() && code_hash == SUDT_CODE_HASH {
                return Ok(type_.calc_script_hash().unpack());
            }
            return Err(anyhow!("invalid SUDT token"));
        }
        None => Ok(CKB_TOKEN_ID),
    }
}

fn fetch_sudt_value(token_id: &[u8; 32], output: &CellOutput, data: &[u8]) -> u128 {
    if token_id == &CKB_TOKEN_ID {
        let capacity: u64 = output.capacity().unpack();
        return capacity.into();
    }
    let mut buf = [0u8; 16];
    buf.copy_from_slice(&data[..16]);
    u128::from_le_bytes(buf)
}
