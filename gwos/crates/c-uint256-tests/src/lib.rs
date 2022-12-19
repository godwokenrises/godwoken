use std::cmp::Ordering;

use self::bindings::{
    gw_uint256_cmp, gw_uint256_one, gw_uint256_overflow_add, gw_uint256_underflow_sub, uint256_t,
    GW_UINT256_EQUAL, GW_UINT256_LARGER, GW_UINT256_SMALLER,
};

// deref_nullptr in test code `fn bindgen_test_layout_uint256_t()`.
#[allow(dead_code)]
#[allow(non_snake_case)]
#[allow(non_camel_case_types)]
#[allow(non_upper_case_globals)]
#[allow(deref_nullptr)]
mod bindings;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct U256(uint256_t);

impl U256 {
    pub fn from_le_bytes(bytes: [u8; 32]) -> Self {
        let mut array = [0u32; 8];
        for (i, v) in array.iter_mut().enumerate() {
            let start = i * 4;
            let end = i * 4 + 4;
            let u32_val = u32::from_le_bytes(bytes[start..end].try_into().unwrap());
            *v = u32_val;
        }
        U256(uint256_t { array })
    }

    pub fn to_le_bytes(&self) -> [u8; 32] {
        let mut bytes = [0u8; 32];
        for i in 0..8 {
            let u32_val = self.0.array[i];
            let start = i * 4;
            let end = i * 4 + 4;
            bytes[start..end].copy_from_slice(&u32_val.to_le_bytes());
        }

        bytes
    }

    pub fn zero() -> Self {
        U256(uint256_t { array: [0u32; 8] })
    }

    pub fn one() -> Self {
        let mut val = Self::zero();
        unsafe { gw_uint256_one(&mut val.0) };

        val
    }

    pub fn checked_add(&self, other: U256) -> Option<U256> {
        let mut sum = U256::zero();
        match unsafe { gw_uint256_overflow_add(self.0, other.0, &mut sum.0) } {
            0 => Some(sum),
            _err => None,
        }
    }

    pub fn checked_sub(&self, other: U256) -> Option<U256> {
        let mut rem = U256::zero();
        match unsafe { gw_uint256_underflow_sub(self.0, other.0, &mut rem.0) } {
            0 => Some(rem),
            _err => None,
        }
    }
}

impl PartialOrd for U256 {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        match unsafe { gw_uint256_cmp(self.0, other.0) } {
            GW_UINT256_SMALLER => Some(Ordering::Less),
            GW_UINT256_EQUAL => Some(Ordering::Equal),
            GW_UINT256_LARGER => Some(Ordering::Greater),
            _ => None,
        }
    }
}

impl Ord for U256 {
    fn cmp(&self, other: &Self) -> Ordering {
        self.partial_cmp(other).unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::U256 as CU256;
    use primitive_types::U256 as PU256;
    use proptest::prelude::*;

    impl PartialEq<PU256> for CU256 {
        fn eq(&self, other: &PU256) -> bool {
            let mut other_le_bytes = [0u8; 32];
            other.to_little_endian(&mut other_le_bytes);

            self.to_le_bytes() == other_le_bytes
        }
    }

    impl CU256 {
        fn into_pu256(self) -> PU256 {
            PU256::from_little_endian(&self.to_le_bytes())
        }
    }

    #[test]
    fn test_c_uint256_one() {
        let one = CU256::one();
        assert_eq!(one.checked_sub(one), Some(CU256::zero()));

        let p_one = PU256::one();
        assert_eq!(one, p_one);
    }

    proptest! {
        #[test]
        fn test_c_uint256_checked_add(
            a in prop::array::uniform32(any::<u8>()),
            b in prop::array::uniform32(any::<u8>())
        ) {
            let ca = CU256::from_le_bytes(a);
            let cb = CU256::from_le_bytes(b);
            let csum = ca.checked_add(cb);

            let pa = PU256::from_little_endian(&a);
            let pb = PU256::from_little_endian(&b);
            let psum = pa.checked_add(pb);
            prop_assert_eq!(csum.map(CU256::into_pu256), psum);
        }

        #[test]
        fn test_c_uint256_checked_sub(
            a in prop::array::uniform32(any::<u8>()),
            b in prop::array::uniform32(any::<u8>())
        ) {
            let ca = CU256::from_le_bytes(a);
            let cb = CU256::from_le_bytes(b);
            let crem = ca.checked_sub(cb);

            let pa = PU256::from_little_endian(&a);
            let pb = PU256::from_little_endian(&b);
            let prem = pa.checked_sub(pb);
            prop_assert_eq!(crem.map(CU256::into_pu256), prem);
        }

        #[test]
        fn test_c_uint256_cmp(
            a in prop::array::uniform32(any::<u8>()),
            b in prop::array::uniform32(any::<u8>())
        ) {
            let ca = CU256::from_le_bytes(a);
            let cb = CU256::from_le_bytes(b);

            let pa = PU256::from_little_endian(&a);
            let pb = PU256::from_little_endian(&b);

            prop_assert_eq!(ca > cb, pa > pb);
        }
    }
}
