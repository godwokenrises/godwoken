use crate::{packed, prelude::*, vec::Vec};

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

impl Pack<packed::KVPair> for ([u8; 32], [u8; 32]) {
    fn pack(&self) -> packed::KVPair {
        packed::KVPair::new_builder()
            .k(self.0.pack())
            .v(self.1.pack())
            .build()
    }
}

impl_conversion_for_entity_unpack!(([u8; 32], [u8; 32]), KVPair);

impl<'r> Unpack<([u8; 32], [u8; 32])> for packed::KVPairReader<'r> {
    fn unpack(&self) -> ([u8; 32], [u8; 32]) {
        (self.k().unpack(), self.v().unpack())
    }
}

impl_conversion_for_vector!(([u8; 32], [u8; 32]), KVPairVec, KVPairVecReader);
impl_conversion_for_packed_iterator_pack!(KVPair, KVPairVec);
impl_conversion_for_packed_iterator_pack!(DepositionRequest, DepositionRequestVec);
impl_conversion_for_packed_iterator_pack!(WithdrawalRequest, WithdrawalRequestVec);
impl_conversion_for_packed_iterator_pack!(L2Transaction, L2TransactionVec);
impl_conversion_for_packed_iterator_pack!(LogItem, LogItemVec);
