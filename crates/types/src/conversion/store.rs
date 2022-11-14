use std::convert::TryInto;

use crate::{packed, prelude::*};

impl Pack<packed::TransactionKey> for [u8; 36] {
    fn pack(&self) -> packed::TransactionKey {
        packed::TransactionKey::from_slice(&self[..]).expect("impossible: fail to pack [u8; 36]")
    }
}

impl<'r> Unpack<[u8; 36]> for packed::TransactionKeyReader<'r> {
    #[inline]
    fn unpack(&self) -> [u8; 36] {
        self.as_slice()
            .try_into()
            .expect("unpack TransactionKeyReader")
    }
}
impl_conversion_for_entity_unpack!([u8; 36], TransactionKey);

impl Pack<packed::WithdrawalKey> for [u8; 36] {
    fn pack(&self) -> packed::WithdrawalKey {
        packed::WithdrawalKey::from_slice(&self[..]).expect("impossible: fail to pack [u8; 36]")
    }
}

impl<'r> Unpack<[u8; 36]> for packed::WithdrawalKeyReader<'r> {
    #[inline]
    fn unpack(&self) -> [u8; 36] {
        self.as_slice()
            .try_into()
            .expect("unpack WithdrawalKeyReader")
    }
}
impl_conversion_for_entity_unpack!([u8; 36], WithdrawalKey);

impl_conversion_for_packed_iterator_pack!(LogItem, LogItemVec);
