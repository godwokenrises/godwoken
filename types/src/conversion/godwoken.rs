use crate::{packed, prelude::*};

impl Pack<packed::Byte20> for [u8; 20] {
    fn pack(&self) -> packed::Byte20 {
        packed::Byte20::from_slice(&self[..]).expect("impossible: fail to pack [u8; 20]")
    }
}

impl<'r> Unpack<[u8; 20]> for packed::Byte20Reader<'r> {
    fn unpack(&self) -> [u8; 20] {
        let ptr = self.as_slice().as_ptr() as *const [u8; 20];
        unsafe { *ptr }
    }
}
impl_conversion_for_entity_unpack!([u8; 20], Byte20);

impl Pack<packed::Signature> for [u8; 65] {
    fn pack(&self) -> packed::Signature {
        packed::Signature::from_slice(&self[..]).expect("impossible: fail to pack [u8; 65]")
    }
}

impl<'r> Unpack<[u8; 65]> for packed::SignatureReader<'r> {
    fn unpack(&self) -> [u8; 65] {
        let ptr = self.as_slice().as_ptr() as *const [u8; 65];
        unsafe { *ptr }
    }
}
impl_conversion_for_entity_unpack!([u8; 65], Signature);
