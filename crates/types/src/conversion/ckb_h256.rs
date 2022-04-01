use std::convert::TryInto;

use crate::{packed, prelude::*};
use ckb_fixed_hash::H256;

impl Pack<packed::Byte32> for H256 {
    fn pack(&self) -> packed::Byte32 {
        packed::Byte32::from_slice(&self.0).expect("impossible: fail to pack CKB H256")
    }
}

impl<'r> Unpack<H256> for packed::Byte32Reader<'r> {
    #[inline]
    fn unpack(&self) -> H256 {
        let r: [u8; 32] = self.as_slice().try_into().expect("unpack Byte32Reader");
        r.into()
    }
}
impl_conversion_for_entity_unpack!(H256, Byte32);
