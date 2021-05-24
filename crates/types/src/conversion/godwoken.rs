use crate::{core::H256, packed, prelude::*, vec::Vec};

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

impl Pack<packed::Symbol> for [u8; 8] {
    fn pack(&self) -> packed::Symbol {
        packed::Symbol::from_slice(&self[..]).expect("impossible: fail to pack [u8; 8]")
    }
}

impl<'r> Unpack<[u8; 8]> for packed::SymbolReader<'r> {
    fn unpack(&self) -> [u8; 8] {
        let ptr = self.as_slice().as_ptr() as *const [u8; 8];
        unsafe { *ptr }
    }
}
impl_conversion_for_entity_unpack!([u8; 8], Symbol);

impl Pack<packed::KVPair> for (H256, H256) {
    fn pack(&self) -> packed::KVPair {
        packed::KVPair::new_builder()
            .k(self.0.pack())
            .v(self.1.pack())
            .build()
    }
}

impl_conversion_for_entity_unpack!((H256, H256), KVPair);

impl<'r> Unpack<(H256, H256)> for packed::KVPairReader<'r> {
    fn unpack(&self) -> (H256, H256) {
        (self.k().unpack(), self.v().unpack())
    }
}

impl_conversion_for_vector!((H256, H256), KVPairVec, KVPairVecReader);
impl_conversion_for_packed_iterator_pack!(KVPair, KVPairVec);
impl_conversion_for_packed_iterator_pack!(DepositionRequest, DepositionRequestVec);
impl_conversion_for_packed_iterator_pack!(WithdrawalRequest, WithdrawalRequestVec);
impl_conversion_for_packed_iterator_pack!(L2Transaction, L2TransactionVec);
impl_conversion_for_packed_iterator_pack!(RawL2Block, RawL2BlockVec);
impl_conversion_for_packed_iterator_pack!(AllowedScript, AllowedScriptVec);
