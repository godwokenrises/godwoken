use crate::{packed, prelude::*, vec::Vec};
use sparse_merkle_tree::H256;

impl Pack<packed::Byte32> for H256 {
    fn pack(&self) -> packed::Byte32 {
        packed::Byte32::from_slice(self.as_slice()).expect("impossible: fail to pack CKB H256")
    }
}

impl<'r> Unpack<H256> for packed::Byte32Reader<'r> {
    fn unpack(&self) -> H256 {
        let ptr = self.as_slice().as_ptr() as *const [u8; 32];
        let r = unsafe { *ptr };
        r.into()
    }
}
impl_conversion_for_entity_unpack!(H256, Byte32);

impl_conversion_for_vector!(H256, Byte32Vec, Byte32VecReader);
