use ckb_fixed_hash::H256 as JsonH256;
use gw_common::H256;

pub(crate) fn to_h256(v: JsonH256) -> H256 {
    let h: [u8; 32] = v.into();
    h.into()
}

pub(crate) fn to_jsonh256(v: H256) -> JsonH256 {
    let h: [u8; 32] = v.into();
    h.into()
}
