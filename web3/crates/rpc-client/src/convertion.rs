use ckb_jsonrpc_types::Script;
use ckb_types::prelude::Entity;
use gw_jsonrpc_types::godwoken::{L2Block, L2BlockView};
use gw_types::packed;

pub fn to_l2_block(l2_block_view: L2BlockView) -> packed::L2Block {
    let v = l2_block_view;
    let l2_block = L2Block {
        raw: v.raw,
        kv_state: v.kv_state,
        kv_state_proof: v.kv_state_proof,
        transactions: v.transactions.into_iter().map(|tx| tx.inner).collect(),
        block_proof: v.block_proof,
        withdrawals: v.withdrawal_requests,
    };

    l2_block.into()
}

pub fn to_script(script: Script) -> packed::Script {
    let ckb_packed_script: ckb_types::packed::Script = script.into();
    packed::Script::new_unchecked(ckb_packed_script.as_bytes())
}
