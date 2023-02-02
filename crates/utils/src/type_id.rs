use ckb_chain_spec::consensus::TYPE_ID_CODE_HASH;
use gw_common::blake2b::new_blake2b;
use gw_types::{bytes::Bytes, core::ScriptHashType, packed, prelude::*};

pub fn type_id_type_script(
    first_cell_input: packed::CellInputReader,
    output_index: u64,
) -> packed::Script {
    packed::Script::new_builder()
        .code_hash(TYPE_ID_CODE_HASH.pack())
        .hash_type(ScriptHashType::Type.into())
        .args(type_id_args(first_cell_input, output_index).pack())
        .build()
}

pub fn type_id_args(first_cell_input: packed::CellInputReader, output_index: u64) -> Bytes {
    let mut blake2b = new_blake2b();
    blake2b.update(first_cell_input.as_slice());
    blake2b.update(&output_index.to_le_bytes());
    let mut ret = [0; 32];
    blake2b.finalize(&mut ret);
    Bytes::from(ret.to_vec())
}
