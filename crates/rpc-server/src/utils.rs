use ckb_fixed_hash::H256 as JsonH256;
use gw_types::h256::H256;

#[inline]
pub(crate) fn to_h256(v: JsonH256) -> H256 {
    v.into()
}

#[inline]
pub(crate) fn to_jsonh256(v: H256) -> JsonH256 {
    v.into()
}
