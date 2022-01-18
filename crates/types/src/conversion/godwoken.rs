use crate::{core::H256, packed, prelude::*, vec::Vec};

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

impl_conversion_for_vector!(u32, Uint32Vec, Uint32VecReader);
impl_conversion_for_vector!((H256, H256), KVPairVec, KVPairVecReader);
impl_conversion_for_packed_iterator_pack!(KVPair, KVPairVec);
impl_conversion_for_packed_iterator_pack!(DepositRequest, DepositRequestVec);
impl_conversion_for_packed_iterator_pack!(WithdrawalRequest, WithdrawalRequestVec);
impl_conversion_for_packed_iterator_pack!(L2Transaction, L2TransactionVec);
impl_conversion_for_packed_iterator_pack!(RawL2Block, RawL2BlockVec);
