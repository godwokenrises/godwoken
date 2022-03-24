use std::convert::TryInto;

use crate::{borrow::ToOwned, str, string::String, vec::Vec};
use crate::{bytes::Bytes, packed, prelude::*};

impl Pack<packed::Uint16> for u16 {
    fn pack(&self) -> packed::Uint16 {
        packed::Uint16::new_unchecked(Bytes::from(self.to_le_bytes().to_vec()))
    }
}

impl Pack<packed::Uint32> for u32 {
    fn pack(&self) -> packed::Uint32 {
        packed::Uint32::new_unchecked(Bytes::from(self.to_le_bytes().to_vec()))
    }
}

impl Pack<packed::Uint64> for u64 {
    fn pack(&self) -> packed::Uint64 {
        packed::Uint64::new_unchecked(Bytes::from(self.to_le_bytes().to_vec()))
    }
}

impl Pack<packed::Uint128> for u128 {
    fn pack(&self) -> packed::Uint128 {
        packed::Uint128::new_unchecked(Bytes::from(self.to_le_bytes().to_vec()))
    }
}

impl Pack<packed::Uint32> for usize {
    fn pack(&self) -> packed::Uint32 {
        (*self as u32).pack()
    }
}

impl<'r> Unpack<u16> for packed::Uint16Reader<'r> {
    // Inline so that the panic branch can be optimized out.
    #[inline]
    fn unpack(&self) -> u16 {
        // Unwrap is ok because slice should always be of the correct length, so try_into should not fail.
        u16::from_le_bytes(self.as_slice().try_into().unwrap())
    }
}
impl_conversion_for_entity_unpack!(u16, Uint16);

impl<'r> Unpack<u32> for packed::Uint32Reader<'r> {
    #[inline]
    fn unpack(&self) -> u32 {
        u32::from_le_bytes(self.as_slice().try_into().unwrap())
    }
}
impl_conversion_for_entity_unpack!(u32, Uint32);

impl<'r> Unpack<u64> for packed::Uint64Reader<'r> {
    #[inline]
    fn unpack(&self) -> u64 {
        u64::from_le_bytes(self.as_slice().try_into().unwrap())
    }
}
impl_conversion_for_entity_unpack!(u64, Uint64);

impl<'r> Unpack<u128> for packed::Uint128Reader<'r> {
    #[inline]
    fn unpack(&self) -> u128 {
        u128::from_le_bytes(self.as_slice().try_into().unwrap())
    }
}
impl_conversion_for_entity_unpack!(u128, Uint128);

impl<'r> Unpack<usize> for packed::Uint32Reader<'r> {
    fn unpack(&self) -> usize {
        let x: u32 = self.unpack();
        x as usize
    }
}
impl_conversion_for_entity_unpack!(usize, Uint32);

impl Pack<packed::Bytes> for [u8] {
    fn pack(&self) -> packed::Bytes {
        let len = self.len();
        let mut vec: Vec<u8> = Vec::with_capacity(4 + len);
        vec.extend_from_slice(&(len as u32).to_le_bytes()[..]);
        vec.extend_from_slice(self);
        packed::Bytes::new_unchecked(Bytes::from(vec))
    }
}

impl<'r> Unpack<Vec<u8>> for packed::BytesReader<'r> {
    fn unpack(&self) -> Vec<u8> {
        self.raw_data().to_owned()
    }
}
impl_conversion_for_entity_unpack!(Vec<u8>, Bytes);

impl Pack<packed::Bytes> for str {
    fn pack(&self) -> packed::Bytes {
        self.as_bytes().pack()
    }
}

impl<'r> packed::BytesReader<'r> {
    pub fn as_utf8(&self) -> Result<&str, str::Utf8Error> {
        str::from_utf8(self.raw_data())
    }

    #[allow(clippy::missing_safety_doc)]
    pub unsafe fn as_utf8_unchecked(&self) -> &str {
        str::from_utf8_unchecked(self.raw_data())
    }

    pub fn is_utf8(&self) -> bool {
        self.as_utf8().is_ok()
    }
}

impl Pack<packed::Bytes> for String {
    fn pack(&self) -> packed::Bytes {
        self.as_str().pack()
    }
}

impl_conversion_for_option_pack!(&str, BytesOpt);
impl_conversion_for_option_pack!(String, BytesOpt);
impl_conversion_for_option_pack!(Bytes, BytesOpt);
