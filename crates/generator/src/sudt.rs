use gw_common::H256;
use gw_types::{
    bytes::Bytes, core::ScriptHashType, offchain::RollupContext, packed::Script, prelude::*,
};

pub fn build_l2_sudt_script(rollup_context: &RollupContext, l1_sudt_script_hash: &H256) -> Script {
    let args = {
        let mut args = Vec::with_capacity(64);
        args.extend(rollup_context.rollup_script_hash.as_slice());
        args.extend(l1_sudt_script_hash.as_slice());
        Bytes::from(args)
    };
    Script::new_builder()
        .args(args.pack())
        .code_hash(
            rollup_context
                .rollup_config
                .l2_sudt_validator_script_type_hash(),
        )
        .hash_type(ScriptHashType::Type.into())
        .build()
}
