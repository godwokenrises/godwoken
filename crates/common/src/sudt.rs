use crate::builtin_scripts::SUDT_VALIDATOR_CODE_HASH;
use gw_types::{bytes::Bytes, core::ScriptHashType, packed::Script, prelude::*};

pub fn build_l2_sudt_script(l1_sudt_script_hash: [u8; 32]) -> Script {
    let args = Bytes::from(l1_sudt_script_hash.to_vec());
    Script::new_builder()
        .args(args.pack())
        .code_hash({
            let code_hash: [u8; 32] = (*SUDT_VALIDATOR_CODE_HASH).into();
            code_hash.pack()
        })
        .hash_type(ScriptHashType::Data.into())
        .build()
}
