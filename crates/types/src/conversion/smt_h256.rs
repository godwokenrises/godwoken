use std::convert::TryInto;

use crate::core::H256;
use crate::{packed, prelude::*, vec::Vec};

impl Pack<packed::Byte32> for H256 {
    fn pack(&self) -> packed::Byte32 {
        packed::Byte32::from_slice(self.as_slice()).expect("impossible: fail to pack CKB H256")
    }
}

impl<'r> Unpack<H256> for packed::Byte32Reader<'r> {
    #[inline]
    fn unpack(&self) -> H256 {
        let r: [u8; 32] = self.as_slice().try_into().unwrap();
        r.into()
    }
}

impl_conversion_for_entity_unpack!(H256, Byte32);
impl_conversion_for_vector!(H256, Byte32Vec, Byte32VecReader);
