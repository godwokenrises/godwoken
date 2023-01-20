use alloc::vec::Vec;

use crate::{bytes::Bytes, packed, prelude::*};

impl Pack<packed::Byte32> for [u8; 32] {
    fn pack(&self) -> packed::Byte32 {
        packed::Byte32::from_slice(&self[..]).expect("impossible: fail to pack [u8; 32]")
    }
}

impl<'r> Unpack<[u8; 32]> for packed::Byte32Reader<'r> {
    #[inline]
    fn unpack(&self) -> [u8; 32] {
        self.as_slice().try_into().expect("unpack Byte32Reader")
    }
}
impl_conversion_for_entity_unpack!([u8; 32], Byte32);

impl Pack<packed::ProposalShortId> for [u8; 10] {
    fn pack(&self) -> packed::ProposalShortId {
        packed::ProposalShortId::from_slice(&self[..])
            .expect("impossible: fail to pack to ProposalShortId")
    }
}

impl<'r> Unpack<[u8; 10]> for packed::ProposalShortIdReader<'r> {
    #[inline]
    fn unpack(&self) -> [u8; 10] {
        self.as_slice()
            .try_into()
            .expect("unpack ProposalShortIdReader")
    }
}
impl_conversion_for_entity_unpack!([u8; 10], ProposalShortId);

impl Pack<packed::Bytes> for Bytes {
    fn pack(&self) -> packed::Bytes {
        let len = (self.len() as u32).to_le_bytes();
        let mut v = Vec::with_capacity(4 + self.len());
        v.extend_from_slice(&len[..]);
        v.extend_from_slice(&self[..]);
        packed::Bytes::new_unchecked(v.into())
    }
}

impl<'r> Unpack<Bytes> for packed::BytesReader<'r> {
    fn unpack(&self) -> Bytes {
        Bytes::from(self.raw_data().to_vec())
    }
}

impl Unpack<Bytes> for packed::Bytes {
    fn unpack(&self) -> Bytes {
        self.raw_data()
    }
}

impl_conversion_for_vector!(Bytes, BytesVec, BytesVecReader);
impl_conversion_for_vector!([u8; 32], Byte32Vec, Byte32VecReader);
impl_conversion_for_packed_optional_pack!(Script, ScriptOpt);
impl_conversion_for_packed_optional_pack!(Bytes, BytesOpt);
// impl_conversion_for_packed_iterator_pack!(ProposalShortId, ProposalShortIdVec);
impl_conversion_for_packed_iterator_pack!(Bytes, BytesVec);
// impl_conversion_for_packed_iterator_pack!(Transaction, TransactionVec);
impl_conversion_for_packed_iterator_pack!(CellDep, CellDepVec);
impl_conversion_for_packed_iterator_pack!(CellOutput, CellOutputVec);
impl_conversion_for_packed_iterator_pack!(CellInput, CellInputVec);
// impl_conversion_for_packed_iterator_pack!(UncleBlock, UncleBlockVec);
impl_conversion_for_packed_iterator_pack!(Byte32, Byte32Vec);
