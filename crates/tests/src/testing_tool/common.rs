use ckb_types::prelude::{Builder, Entity};
use gw_common::H256;
use gw_types::core::ScriptHashType;
use gw_types::packed::Script;
use gw_types::prelude::Pack;

use super::chain::ALWAYS_SUCCESS_CODE_HASH;

pub fn random_always_success_script(rollup_script_hash: &H256) -> Script {
    let random_bytes: [u8; 20] = rand::random();
    Script::new_builder()
        .code_hash(ALWAYS_SUCCESS_CODE_HASH.clone().pack())
        .hash_type(ScriptHashType::Type.into())
        .args({
            let mut args = rollup_script_hash.as_slice().to_vec();
            args.extend_from_slice(&random_bytes);
            args.pack()
        })
        .build()
}
